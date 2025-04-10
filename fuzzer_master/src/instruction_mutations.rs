use crate::database::{Database};
use crate::device_connection::DeviceConnection;
use crate::fuzzer_node_bridge::FuzzerNodeInterface;
use crate::{CommandExitResult};
use fuzzer_data::genetic_pool::{GeneticPool};
use fuzzer_data::{
    Code,
};

#[derive(Debug, Default, PartialEq)]
enum FSM {
    #[default]
    Uninitialized,
    Running,
}

#[derive(Default)]
pub struct InstructionMutState {
    fsm: FSM,

    genetic_pool: GeneticPool,
    random_source: Option<rand_isaac::Isaac64Rng>,

    last_reported_exclusion: Option<(Option<u16>, u16)>, // address, times

    evolution: u64,
    seed: u64,

    last_code_executed: Option<Code>,
}

pub const SAMPLE_TIMEOUT: u64 = 60;
pub const MAX_EVOLUTIONS: u64 = 50;

pub async fn main(
    net: &mut DeviceConnection,
    interface: &FuzzerNodeInterface,
    database: &mut Database,
    state: &mut InstructionMutState,
) -> CommandExitResult {
    // device is either restarted or new experimentation run


    CommandExitResult::ExitProgram
}


