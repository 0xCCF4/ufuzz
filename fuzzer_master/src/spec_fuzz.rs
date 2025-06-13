//! Speculative Execution Fuzzing Module
//!
//! This module provides functionality for fuzzing speculative execution behavior, e.g. the main fuzzing loop

use crate::database::Database;
use crate::device_connection::DeviceConnection;
use crate::net::{net_speculative_sample, ExecuteSampleResult};
use crate::CommandExitResult;
use hypervisor::state::StateDifference;
use itertools::Itertools;
use log::{error, info, trace};
use rand::{random, SeedableRng};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs::File;
use std::io::{BufReader, BufWriter, Read};
use std::path::Path;
use ucode_compiler_dynamic::instruction::Instruction;
use ucode_compiler_dynamic::opcodes::Opcode;
use ucode_compiler_dynamic::sequence_word::SequenceWord;
use x86_perf_counter::PerfEventSpecifier;

/// Results of speculative execution
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SpecResult {
    /// Execution timed out
    Timeout,
    /// Execution completed with PMC and architectural state changes
    Executed {
        /// Map of PMC event specifiers to their delta values
        pmc_delta: BTreeMap<StringBox<PerfEventSpecifier>, i64>,
        /// Set of architectural state differences
        arch_delta: BTreeSet<String>,
    },
}

impl SpecResult {
    /// Checks if two results are of the same kind (both timeout or both executed)
    pub fn is_same_kind(&self, other: &Self) -> bool {
        match (self, other) {
            (SpecResult::Timeout, SpecResult::Timeout) => true,
            (SpecResult::Executed { .. }, SpecResult::Executed { .. }) => true,
            _ => false,
        }
    }
}

/// Wrapper for types that need string-based serialization
#[derive(Debug, Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct StringBox<T>(pub T);

impl Serialize for StringBox<Instruction> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        format!(
            "{}|{}|{:x}",
            self.0.assemble_no_crc(),
            self.0.opcode(),
            self.0.assemble_no_crc()
        )
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for StringBox<Instruction> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let text = String::deserialize(deserializer)?;
        let text = text.split('|').next().expect("deserialization error");
        let val = u64::from_str_radix(text, 10).map_err(serde::de::Error::custom)?;
        Ok(StringBox(Instruction::disassemble(val)))
    }
}

impl Serialize for StringBox<PerfEventSpecifier> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serde_json::to_string(&self.0)
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for StringBox<PerfEventSpecifier> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let text = String::deserialize(deserializer)?;
        let val =
            serde_json::from_str::<PerfEventSpecifier>(&text).map_err(serde::de::Error::custom)?;
        Ok(StringBox(val))
    }
}

impl<T> From<T> for StringBox<T> {
    fn from(value: T) -> Self {
        StringBox(value)
    }
}

/// Report of speculative execution findings
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SpecReport {
    /// Map of instructions to their speculative execution results
    pub opcodes: BTreeMap<StringBox<Instruction>, SpecResult>,
    /// Set of blacklisted PMC event select values
    #[serde(default)]
    pub pmc_blacklist_event_select: BTreeSet<u8>,
    /// Map of PMC event specifiers to their baseline values
    #[serde(default)]
    pub pmc_baseline: BTreeMap<StringBox<PerfEventSpecifier>, u64>,
}

impl SpecReport {
    /// Creates a new empty spec report
    pub fn new() -> Self {
        Self {
            opcodes: BTreeMap::new(),
            pmc_blacklist_event_select: BTreeSet::new(),
            pmc_baseline: BTreeMap::default(),
        }
    }

    /// Loads a spec report from a file
    pub fn load_file<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        serde_json::from_reader(BufReader::new(
            std::fs::File::open(&path)
                .map_err(|e| format!("Failed to open file: {:?} - {:?}", path.as_ref(), e))?,
        ))
        .map_err(|e| format!("Failed to deserialize JSON: {:?}", e))
    }

    /// Saves the spec report to a file
    pub fn save_file<P: AsRef<Path>>(&self, path: P) -> Result<(), String> {
        serde_json::to_writer_pretty(
            BufWriter::new(
                std::fs::File::create(&path)
                    .map_err(|e| format!("Failed to create file: {:?} - {:?}", path.as_ref(), e))?,
            ),
            self,
        )
        .map_err(|e| format!("Failed to serialize JSON: {:?}", e))
    }

    /// Adds a result for an instruction to the report
    pub fn add_result(&mut self, instruction: Instruction, result: SpecResult) {
        let value = self
            .opcodes
            .entry(instruction.into())
            .or_insert(result.clone());

        match result {
            SpecResult::Timeout => {
                if let SpecResult::Executed { .. } = value {
                    error!(
                        "Timeout for instruction but it had worked before: {:?}",
                        instruction
                    );
                }
                *value = SpecResult::Timeout;
            }
            SpecResult::Executed {
                arch_delta,
                pmc_delta,
            } => {
                if let SpecResult::Executed {
                    arch_delta: old_arch_delta,
                    pmc_delta: old_pmc_delta,
                } = value
                {
                    old_arch_delta.extend(arch_delta);
                    for (key, delta) in pmc_delta.iter() {
                        *old_pmc_delta.entry(key.clone()).or_insert(0) = *delta;
                    }
                } else {
                    error!(
                        "Executed {:?} but it had worked before: {:?}",
                        instruction, value
                    );
                    *value = SpecResult::Executed {
                        arch_delta,
                        pmc_delta,
                    };
                }
            }
        }
    }
}

/// Finite state machine states for fuzzing process
#[derive(Debug, Default, PartialEq)]
enum FSM {
    /// Initial state before fuzzing starts
    #[default]
    Uninitialized,
    /// Acquiring baseline measurements
    AcquireBaseline,
    /// Fuzzing is in progress
    Running,
}

/// State management for speculative execution fuzzing
#[derive(Default)]
pub struct SpecFuzzMutState {
    /// Current state of the fuzzing process
    fsm: FSM,
    /// Random number generator for mutations
    random_source: Option<rand_isaac::Isaac64Rng>,
    /// List of all microcode instructions
    all_ucodes: Vec<Instruction>,
    /// List of all PMC event specifiers
    all_pmcs: Vec<PerfEventSpecifier>,
    /// List of PMC event specifier groups
    all_pmc_groups: Vec<Vec<PerfEventSpecifier>>,
    /// Current spec report
    report: SpecReport,
    /// Baseline PMC measurements
    baseline: BTreeMap<PerfEventSpecifier, u64>,
    /// Queue of PMC groups to test
    pmc_queue: VecDeque<Vec<PerfEventSpecifier>>,
    /// Queue of microcode instructions to test
    ucode_queue: VecDeque<Instruction>,
    /// Expected RFLAGS value
    expected_rflags: u64,
}

/// Copies a database template to a new location
///
/// # Arguments
///
/// * `source_path` - Path to the source template database
/// * `target_path` - Path for the new database
pub fn copy_database_template<A: AsRef<Path>, B: AsRef<Path>>(source_path: A, target_path: B) {
    let mut report = SpecReport::load_file(source_path.as_ref()).unwrap_or_else(|e| {
        error!("Failed to load database: {}", e);
        SpecReport::default()
    });

    if target_path.as_ref().exists() {
        error!("Target path already exists: {:?}", target_path.as_ref());
        return;
    }

    File::create(target_path.as_ref()).expect("Failed to create target file");
    report.opcodes.clear();
    if let Err(err) = report.save_file(target_path.as_ref()) {
        error!("Failed to save the report: {:?}", err);
    } else {
        info!("Database template copied to {:?}", target_path.as_ref());
    }
}

/// Main entry point for microcode speculative execution fuzzing
pub async fn main<A: AsRef<Path>, B: AsRef<Path>>(
    net: &mut DeviceConnection,
    database: &mut Database,
    state: &mut SpecFuzzMutState,
    report: A,
    all: bool,
    skip: bool,
    no_crbus: bool,
    exclude: Option<B>,
    fuzzy_pmc: bool,
) -> CommandExitResult {
    // device is either restarted or new experimentation run

    match state.fsm {
        FSM::Uninitialized => {
            // new experimentation run
            let seed = random();

            info!("Starting new experimentation run with seed: {}", seed);
            state.random_source = Some(rand_isaac::Isaac64Rng::seed_from_u64(seed));

            state.report = SpecReport::load_file(&report).unwrap_or_default();

            let instructions = ucode_dump::dump::ROM_cpu_000506CA
                .instructions()
                .iter()
                .map(|x| {
                    if all {
                        Instruction::disassemble(*x)
                    } else {
                        Instruction::from_opcode(Instruction::disassemble(*x).opcode())
                    }
                })
                .chain(
                    ucode_dump::dump::ROM_cpu_000506C9
                        .instructions()
                        .iter()
                        .map(|x| {
                            if all {
                                Instruction::disassemble(*x)
                            } else {
                                Instruction::from_opcode(Instruction::disassemble(*x).opcode())
                            }
                        }),
                )
                .unique()
                .sorted_by_key(|x| x.assemble_no_crc());

            let excluded = match exclude {
                None => vec![],
                Some(exclude) => {
                    let mut excluded_text = String::new();
                    File::open(exclude)
                        .expect("File exclude does not exists")
                        .read_to_string(&mut excluded_text)
                        .expect("Failed to read exclude file");
                    let lines = excluded_text.lines();

                    let mut result = Vec::new();
                    for line in lines {
                        if line.is_empty() {
                            continue;
                        }
                        let before_first_comma = line
                            .split(',')
                            .next()
                            .map(|v| v.to_string())
                            .unwrap_or(line.to_string());
                        let parsed_hex_num = u64::from_str_radix(&before_first_comma, 16)
                            .expect("Non hex number in exclude file");
                        let instruction = Instruction::from(parsed_hex_num);
                        result.push(instruction);
                    }
                    result
                }
            };

            for instruction in instructions {
                if !excluded.contains(&instruction) {
                    state.all_ucodes.push(instruction);
                }
            }

            if !fuzzy_pmc {
                state.all_pmcs = vec![
                    x86_perf_counter::INSTRUCTIONS_RETIRED,
                    x86_perf_counter::MS_DECODED_MS_ENTRY,
                    x86_perf_counter::UOPS_ISSUED_ANY,
                    x86_perf_counter::UOPS_RETIRED_ANY,
                ];
            } else {
                for event_select in 0..0xFF {
                    for umask in 0..0xFF {
                        let pmc = PerfEventSpecifier {
                            event_select,
                            umask,
                            edge_detect: None,
                            cmask: None,
                        };
                        state.all_pmcs.push(pmc);
                    }
                }
            }

            state.pmc_queue.clear();
            for pmc in state.all_pmcs.iter() {
                state.pmc_queue.push_back(vec![pmc.clone()]);
            }

            println!("Progressing to ACQUIRE BASELINE phase");
            state.fsm = FSM::AcquireBaseline;
            return CommandExitResult::Operational;
        }
        FSM::AcquireBaseline => {
            while let Some(pmc) = state.pmc_queue.pop_front() {
                println!("Remaining Baseline: {}", state.pmc_queue.len());

                let pmc = pmc.first().unwrap();

                if state
                    .report
                    .pmc_blacklist_event_select
                    .contains(&pmc.event_select)
                {
                    continue;
                }

                if let Some(baseline) = state.report.pmc_baseline.get(&StringBox(*pmc)) {
                    state.baseline.insert(*pmc, *baseline);
                    continue;
                }

                let result = net_speculative_sample(
                    net,
                    [
                        Instruction::from_opcode(Opcode::ADD_DSZ32),
                        Instruction::NOP,
                        Instruction::NOP,
                    ],
                    SequenceWord::NOP,
                    vec![pmc.clone()],
                )
                .await;

                let result = match result {
                    ExecuteSampleResult::Timeout => {
                        error!("Timeout PMC: {:?}", pmc);
                        state
                            .report
                            .pmc_blacklist_event_select
                            .insert(pmc.event_select);
                        if let Err(err) = state.report.save_file(&report) {
                            error!("Failed to save the report: {:?}", err);
                        }
                        return CommandExitResult::ForceReconnect;
                    }
                    ExecuteSampleResult::Rerun => {
                        trace!("Rerunning PMC: {:?}", pmc);
                        state.pmc_queue.push_back(vec![pmc.clone()]);
                        return CommandExitResult::Operational;
                    }
                    ExecuteSampleResult::Success(x) => x,
                };

                state.expected_rflags = result.arch_after.rflags;

                let result = result.perf_counters.iter().next();

                match result {
                    Some(pmc_value) => {
                        state.baseline.insert(pmc.clone(), *pmc_value);
                        state
                            .report
                            .pmc_baseline
                            .insert(StringBox(*pmc), *pmc_value);
                        if let Err(err) = state.report.save_file(&report) {
                            error!("Failed to save the report: {:?}", err);
                        }
                    }
                    None => {
                        error!("PMC value is None for {:?}", pmc);
                    }
                }
            }

            state.ucode_queue.clear();
            state.ucode_queue.extend(&state.all_ucodes);
            state.all_pmc_groups.clear();
            for pmc in &state.baseline.keys().cloned().chunks(4) {
                state.all_pmc_groups.push(pmc.collect_vec());
            }
            state.pmc_queue.clear();
            state.pmc_queue.extend(state.all_pmc_groups.clone());

            info!("Progressing to RUN phase");
            state.fsm = FSM::Running;
            return CommandExitResult::Operational;
        }
        FSM::Running => {
            while let Some(pmc) = state.pmc_queue.front() {
                while let Some(instruction) = state.ucode_queue.front() {
                    trace!(
                        "Remaining: PMC:{} Instructions:{}",
                        state.pmc_queue.len(),
                        state.ucode_queue.len()
                    );
                    if skip {
                        if let Some(entry) = state.report.opcodes.get(&StringBox(*instruction)) {
                            if let SpecResult::Executed { pmc_delta, .. } = entry {
                                if pmc.iter().all(|f| pmc_delta.contains_key(&StringBox(*f))) {
                                    state.ucode_queue.pop_front();
                                    continue;
                                }
                            }
                        }
                    }
                    if no_crbus && instruction.opcode().is_group_MOVETOCREG() {
                        continue;
                    }

                    let result = net_speculative_sample(
                        net,
                        [*instruction, Instruction::NOP, Instruction::NOP],
                        SequenceWord::NOP,
                        vec![
                            x86_perf_counter::INSTRUCTIONS_RETIRED,
                            x86_perf_counter::MS_DECODED_MS_ENTRY,
                            x86_perf_counter::UOPS_ISSUED_ANY,
                            x86_perf_counter::UOPS_RETIRED_ANY,
                        ],
                    )
                    .await;

                    let mut result = match result {
                        ExecuteSampleResult::Timeout => {
                            error!(
                                "Timeout: {} : {:04x}",
                                instruction.opcode(),
                                instruction.assemble()
                            );
                            state.report.add_result(*instruction, SpecResult::Timeout);
                            if let Err(err) = state.report.save_file(&report) {
                                error!("Failed to save the report: {:?}", err);
                            }
                            return CommandExitResult::ForceReconnect;
                        }
                        ExecuteSampleResult::Rerun => {
                            return CommandExitResult::Operational;
                        }
                        ExecuteSampleResult::Success(x) => x,
                    };

                    let mut pmc_delta = BTreeMap::new();
                    for (i, value) in result.perf_counters.iter().enumerate() {
                        if let Some(pmc_key) = pmc.get(i) {
                            if let Some(baseline) = state.baseline.get(pmc_key) {
                                let delta = *value as i64 - *baseline as i64;
                                pmc_delta.insert(StringBox::from(pmc_key.clone()), delta);
                            } else {
                                error!("No baseline for pmc {pmc_key:?}");
                            }
                        } else {
                            error!("unknown pmc index {i}")
                        }
                    }

                    result.arch_before.rflags = state.expected_rflags; // todo: hacky

                    let arch_delta = result.arch_before.difference(&result.arch_after);

                    if arch_delta.len() > 0 {
                        error!(
                            "Arch state mismatch: {} : {:04x}",
                            instruction.opcode(),
                            instruction.assemble()
                        );
                        for (name, before, after) in arch_delta.iter() {
                            error!("Arch state mismatch: {}: {:?} -> {:?}", name, before, after);
                        }
                    }

                    state.report.add_result(
                        *instruction,
                        SpecResult::Executed {
                            pmc_delta,
                            arch_delta: arch_delta
                                .iter()
                                .map(|(name, _, _)| name.to_string())
                                .collect::<BTreeSet<String>>(),
                        },
                    );

                    if let Err(err) = state.report.save_file(&report) {
                        error!("Failed to save the report: {:?}", err);
                    }

                    let _ = state.ucode_queue.pop_front();
                }

                let _ = state.pmc_queue.pop_front();
                state.ucode_queue.clear();
                state.ucode_queue.extend(&state.all_ucodes);
            }

            info!("Finished execution")
        }
    }

    let _ = database.save().await.map_err(|e| {
        error!("Failed to save the database: {:?}", e);
    });

    CommandExitResult::ExitProgram
}
