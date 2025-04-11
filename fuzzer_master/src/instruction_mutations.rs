use crate::database::Database;
use crate::device_connection::DeviceConnection;
use crate::fuzzer_node_bridge::FuzzerNodeInterface;
use crate::net::net_fuzzing_pretext;
use crate::CommandExitResult;
use fuzzer_data::Code;
use log::{error, info};
use rand::{random, SeedableRng};

#[derive(Debug, Default, PartialEq)]
enum FSM {
    #[default]
    Uninitialized,
    Running,
}

#[derive(Default)]
pub struct InstructionMutState {
    fsm: FSM,

    random_source: Option<rand_isaac::Isaac64Rng>,

    last_reported_exclusion: Option<(Option<u16>, u16)>, // address, times

    evolution: u64,
    seed: u64,

    last_code_executed: Option<Code>,
}

pub async fn main(
    net: &mut DeviceConnection,
    interface: &FuzzerNodeInterface,
    database: &mut Database,
    state: &mut InstructionMutState,
) -> CommandExitResult {
    // device is either restarted or new experimentation run

    match state.fsm {
        FSM::Uninitialized => {
            // new experimentation run
            let seed = random();

            info!("Starting new experimentation run with seed: {}", seed);
            state.random_source = Some(rand_isaac::Isaac64Rng::seed_from_u64(seed));
            state.seed = seed;
            state.evolution = 1;
            state.last_code_executed = None;
        }
        FSM::Running => {
            // device was restarted
        }
    }

    let result = net_fuzzing_pretext(
        net,
        database,
        &mut state.last_code_executed,
        &mut state.last_reported_exclusion,
    )
    .await;
    if result != CommandExitResult::Operational {
        return result;
    }

    if state.fsm == FSM::Uninitialized {
        // initialize

        state.fsm = FSM::Running;
    }

    if state.fsm == FSM::Running {}

    let _ = database.save().await.map_err(|e| {
        error!("Failed to save the database: {:?}", e);
    });

    CommandExitResult::ExitProgram
}
