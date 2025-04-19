use crate::database::Database;
use crate::device_connection::DeviceConnection;
use crate::net::{net_speculative_sample, ExecuteSampleResult};
use crate::CommandExitResult;
use itertools::Itertools;
use log::{error, info};
use rand::{random, SeedableRng};
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
                ExecuteSampleResult::Timeout => return CommandExitResult::ForceReconnect,
                ExecuteSampleResult::Rerun => return CommandExitResult::Operational,
                ExecuteSampleResult::Success(x) => x,
            };

            println!("Result: {} {:?}", instruction.opcode(), result);
        }
    }

    let _ = database.save().await.map_err(|e| {
        error!("Failed to save the database: {:?}", e);
    });

    CommandExitResult::ExitProgram
}
