use crate::database::Database;
use crate::device_connection::DeviceConnection;
use crate::net::{net_speculative_sample, ExecuteSampleResult};
use crate::CommandExitResult;
use itertools::Itertools;
use log::{error, info};
use rand::{random, SeedableRng};
use hypervisor::state::StateDifference;
use ucode_compiler_dynamic::instruction::Instruction;
use ucode_compiler_dynamic::sequence_word::SequenceWord;

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
}

pub async fn main(
    net: &mut DeviceConnection,
    database: &mut Database,
    state: &mut SpecFuzzMutState,
) -> CommandExitResult {
    // device is either restarted or new experimentation run

    match state.fsm {
        FSM::Uninitialized => {
            // new experimentation run
            let seed = random();

            info!("Starting new experimentation run with seed: {}", seed);
            state.random_source = Some(rand_isaac::Isaac64Rng::seed_from_u64(seed));
        }
        FSM::Running => {
            // device was restarted
        }
    }

    if state.fsm == FSM::Uninitialized {

        let result = net_speculative_sample(
            net,
            [Instruction::NOP, Instruction::NOP, Instruction::NOP],
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
            .chain(ucode_dump::dump::ROM_cpu_000506C9.instructions().iter())
            .unique()
            .sorted();
        for instruction in instructions {
            state
                .ucode_queue
                .push(Instruction::disassemble(*instruction));
        }



        state.fsm = FSM::Running;
    }

    if state.fsm == FSM::Running {
        while let Some(instruction) = state.ucode_queue.pop() {

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
                    error!("Timeout: {} : {:04x}", instruction.opcode(), instruction.assemble());
                    return CommandExitResult::ForceReconnect
                },
                ExecuteSampleResult::Rerun => {
                    state.ucode_queue.push(instruction);
                    return CommandExitResult::Operational
                },
                ExecuteSampleResult::Success(x) => x,
            };

            if result.perf_counters != state.baseline {
                error!("Perf counter mismatch: {:?} : {} : {:04x}", result, instruction.opcode(), instruction.assemble());
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

            if result.arch_before != result.arch_after {
                error!("Arch state mismatch: {} : {:04x}", instruction.opcode(), instruction.assemble());
                for (name, before, after) in result.arch_before.difference(&result.arch_after) {
                    error!(
                        "Arch state mismatch: {}: {:?} -> {:?}",
                        name, before, after
                    );
                }
            }
        }
    }

    let _ = database.save().await.map_err(|e| {
        error!("Failed to save the database: {:?}", e);
    });

    CommandExitResult::ExitProgram
}
