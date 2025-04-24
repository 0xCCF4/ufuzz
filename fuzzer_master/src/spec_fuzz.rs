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
use std::io::{BufReader, BufWriter};
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


#[derive(Debug, Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct StringBox<T>(pub T);

impl Serialize for StringBox<Instruction> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        format!("{}|{}|{:x}", self.0.assemble_no_crc(), self.0.opcode(), self.0.assemble_no_crc()).serialize(serializer)
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
        serde_json::to_string(&self.0).map_err(serde::ser::Error::custom)?.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for StringBox<PerfEventSpecifier> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let text = String::deserialize(deserializer)?;
        let val = serde_json::from_str::<PerfEventSpecifier>(&text)
            .map_err(serde::de::Error::custom)?;
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
}

impl SpecReport {
    pub fn new() -> Self {
        Self {
            opcodes: BTreeMap::new(),
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
    Running,
}

#[derive(Default)]
pub struct SpecFuzzMutState {
    fsm: FSM,

    random_source: Option<rand_isaac::Isaac64Rng>,

    ucode_queue: Vec<Instruction>,
    baseline: Vec<u64>,

    report: SpecReport,
}

pub async fn main<A: AsRef<Path>>(
    net: &mut DeviceConnection,
    database: &mut Database,
    state: &mut SpecFuzzMutState,
    report: A,
    all: bool,
    skip: bool,
    no_crbus: bool,
) -> CommandExitResult {
    // device is either restarted or new experimentation run

    match state.fsm {
        FSM::Uninitialized => {
            // new experimentation run
            let seed = random();

            info!("Starting new experimentation run with seed: {}", seed);
            state.random_source = Some(rand_isaac::Isaac64Rng::seed_from_u64(seed));

            state.report = SpecReport::load_file(&report).unwrap_or_default();
        }
        FSM::Running => {
            // device was restarted
        }
    }

    if state.fsm == FSM::Uninitialized {
        let result = net_speculative_sample(
            net,
            [Instruction::from_opcode(Opcode::ADD_DSZ32), Instruction::NOP, Instruction::NOP],
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
            ExecuteSampleResult::Timeout => return CommandExitResult::ForceReconnect,
            ExecuteSampleResult::Rerun => return CommandExitResult::Operational,
            ExecuteSampleResult::Success(x) => x,
        };

        info!("baseline: {:?}", result.perf_counters);
        state.baseline = result.perf_counters;

        // initialize
        let instructions = ucode_dump::dump::ROM_cpu_000506CA
            .instructions()
            .iter()
            .map(|x| if all {
                Instruction::disassemble(*x)
            } else {
                Instruction::from_opcode(Instruction::disassemble(*x).opcode())
            })
            .chain(
                ucode_dump::dump::ROM_cpu_000506C9
                    .instructions()
                    .iter()
                    .map(|x| if all {
                        Instruction::disassemble(*x)
                    } else {
                        Instruction::from_opcode(Instruction::disassemble(*x).opcode())
                    }),
            )
            .unique()
            .sorted_by_key(|x|x.assemble_no_crc());
        for instruction in instructions {
            state
                .ucode_queue
                .push(instruction);
        }

        state.fsm = FSM::Running;
    }

    if state.fsm == FSM::Running {
        while let Some(instruction) = state.ucode_queue.pop() {
            println!("Remaining: {}", state.ucode_queue.len());
            if skip && state.report.opcodes.contains_key(&StringBox(instruction)) {
                continue;
            }
            if no_crbus && instruction.opcode().is_group_MOVETOCREG() {
                continue;
            }

            let result = net_speculative_sample(
                net,
                [instruction, Instruction::NOP, Instruction::NOP],
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
                    state.report.add_result(instruction, SpecResult::Timeout);
                    if let Err(err) = state.report.save_file(&report) {
                        error!("Failed to save the report: {:?}", err);
                    }
                    return CommandExitResult::ForceReconnect;
                }
                ExecuteSampleResult::Rerun => {
                    state.ucode_queue.push(instruction);
                    return CommandExitResult::Operational;
                }
                ExecuteSampleResult::Success(x) => x,
            };

            let pmc_delta = result
                .perf_counters
                .iter()
                .zip(state.baseline.iter())
                .zip([
                    x86_perf_counter::INSTRUCTIONS_RETIRED,
                    x86_perf_counter::MS_DECODED_MS_ENTRY,
                    x86_perf_counter::UOPS_ISSUED_ANY,
                    x86_perf_counter::UOPS_RETIRED_ANY,
                ])
                .map(|((result, baseline), key)| (key.into(), *result as i64 - *baseline as i64))
                .collect::<BTreeMap<_, _>>();

            if result.perf_counters != state.baseline {
                if result.perf_counters[0] != state.baseline[0] {
                    error!(
                        "Perf counter mismatch: INSTRUCTIONS_RETIRED: {} -> {}",
                        result.perf_counters[0], state.baseline[0]
                    );
                }
                if result.perf_counters[1] != state.baseline[1] {
                    error!(
                        "Perf counter mismatch: MS_DECODED_MS_ENTRY: {} -> {}",
                        result.perf_counters[1], state.baseline[1]
                    );
                }
                if result.perf_counters[2] != state.baseline[2] {
                    error!(
                        "Perf counter mismatch: UOPS_ISSUED_ANY: {} -> {}",
                        result.perf_counters[2], state.baseline[2]
                    );
                }
                if result.perf_counters[3] != state.baseline[3] {
                    error!(
                        "Perf counter mismatch: UOPS_RETIRED_ANY: {} -> {}",
                        result.perf_counters[3], state.baseline[3]
                    );
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
                instruction,
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

    let _ = database.save().await.map_err(|e| {
        error!("Failed to save the database: {:?}", e);
    });

    CommandExitResult::ExitProgram
}
