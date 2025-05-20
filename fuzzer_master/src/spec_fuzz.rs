use crate::database::Database;
use crate::device_connection::DeviceConnection;
use crate::net::{net_speculative_sample, ExecuteSampleResult};
use crate::CommandExitResult;
use hypervisor::state::StateDifference;
use itertools::Itertools;
use log::{error, info};
use rand::{random, SeedableRng};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufReader, BufWriter, Read};
use std::path::Path;
use ucode_compiler_dynamic::instruction::Instruction;
use ucode_compiler_dynamic::opcodes::Opcode;
use ucode_compiler_dynamic::sequence_word::SequenceWord;
use x86_perf_counter::PerfEventSpecifier;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SpecResult {
    Timeout,
    Executed {
        pmc_delta: BTreeMap<StringBox<PerfEventSpecifier>, i64>, // pmc setup -> delta
        arch_delta: BTreeSet<String>,
    },
}

impl SpecResult {
    pub fn is_same_kind(&self, other: &Self) -> bool {
        match (self, other) {
            (SpecResult::Timeout, SpecResult::Timeout) => true,
            (SpecResult::Executed { .. }, SpecResult::Executed { .. }) => true,
            _ => false,
        }
    }
}

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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SpecReport {
    pub opcodes: BTreeMap<StringBox<Instruction>, SpecResult>,
    #[serde(default)]
    pub pmc_blacklist_event_select: BTreeSet<u8>,
}

impl SpecReport {
    pub fn new() -> Self {
        Self {
            opcodes: BTreeMap::new(),
            pmc_blacklist_event_select: BTreeSet::new(),
        }
    }

    pub fn load_file<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        serde_json::from_reader(BufReader::new(
            std::fs::File::open(&path)
                .map_err(|e| format!("Failed to open file: {:?} - {:?}", path.as_ref(), e))?,
        ))
        .map_err(|e| format!("Failed to deserialize JSON: {:?}", e))
    }

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

#[derive(Debug, Default, PartialEq)]
enum FSM {
    #[default]
    Uninitialized,
    AcquireBaseline,
    Running,
}

#[derive(Default)]
pub struct SpecFuzzMutState {
    fsm: FSM,

    random_source: Option<rand_isaac::Isaac64Rng>,

    all_ucodes: Vec<Instruction>,
    all_pmcs: Vec<PerfEventSpecifier>,
    all_pmc_groups: Vec<Vec<PerfEventSpecifier>>,

    report: SpecReport,

    baseline: BTreeMap<PerfEventSpecifier, u64>,

    pmc_queue: Vec<Vec<PerfEventSpecifier>>,
    ucode_queue: Vec<Instruction>,
}

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
                state.pmc_queue.push(vec![pmc.clone()]);
            }

            state.fsm = FSM::AcquireBaseline;
            return CommandExitResult::Operational;
        }
        FSM::AcquireBaseline => {
            while let Some(pmc) = state.pmc_queue.pop() {
                println!("Remaining Baseline: {}", state.ucode_queue.len());

                let pmc = pmc.first().unwrap();

                if state
                    .report
                    .pmc_blacklist_event_select
                    .contains(&pmc.event_select)
                {
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
                        state.pmc_queue.push(vec![pmc.clone()]);
                        return CommandExitResult::Operational;
                    }
                    ExecuteSampleResult::Success(x) => x,
                };

                let result = result.perf_counters.iter().next();

                match result {
                    Some(pmc_value) => {
                        state.baseline.insert(pmc.clone(), *pmc_value);
                    }
                    None => {
                        error!("PMC value is None for {:?}", pmc);
                    }
                }
            }

            state.ucode_queue.clone_from(&state.all_ucodes);
            state.all_pmc_groups.clear();
            for pmc in &state.baseline.keys().cloned().chunks(4) {
                state.all_pmc_groups.push(pmc.collect_vec());
            }
            state.pmc_queue.clone_from(&state.all_pmc_groups);

            state.fsm = FSM::Running;
            return CommandExitResult::Operational;
        }
        FSM::Running => {
            while let Some(pmc) = state.pmc_queue.first() {
                while let Some(instruction) = state.ucode_queue.first() {
                    if skip {
                        if let Some(entry) = state.report.opcodes.get(&StringBox(*instruction)) {
                            if let SpecResult::Executed { pmc_delta, .. } = entry {
                                if pmc.iter().all(|f| pmc_delta.contains_key(&StringBox(*f))) {
                                    state.ucode_queue.pop();
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

                    let result = match result {
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
                }
            }
        }
    }

    let _ = database.save().await.map_err(|e| {
        error!("Failed to save the database: {:?}", e);
    });

    CommandExitResult::ExitProgram
}
