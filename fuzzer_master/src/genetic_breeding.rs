use crate::database::{Database, ExcludeType};
use crate::device_connection::DeviceConnection;
use crate::fuzzer_node_bridge::FuzzerNodeInterface;
use crate::{CommandExitResult};
use fuzzer_data::genetic_pool::{GeneticPool, GeneticPoolSettings, GeneticSampleRating};
use fuzzer_data::{
    Code, ExecutionResult, OtaC2DTransport, ReportExecutionProblem,
};
use log::{error, info};
use rand::{random, SeedableRng};
use crate::net::{net_blacklist, net_execute_sample, net_receive_excluded_addresses};

#[derive(Debug, Default, PartialEq)]
enum FSM {
    #[default]
    Uninitialized,
    Running,
}

#[derive(Default)]
pub struct BreedingState {
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
    state: &mut BreedingState,
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

    if let Err(err) = net
        .send(OtaC2DTransport::SetRandomSeed { seed: random() })
        .await
    {
        error!("Failed to set random seed on device: {:?}", err);
        return CommandExitResult::RetryOrReconnect;
    }

    if !net_blacklist(net, database).await {
        return CommandExitResult::RetryOrReconnect;
    }

    if let Err(err) = net
        .send(OtaC2DTransport::DidYouExcludeAnAddressLastRun)
        .await
    {
        error!(
            "Failed to ask device if it excluded an address last run: {:?}",
            err
        );
        return CommandExitResult::RetryOrReconnect;
    } else {
        let address = net_receive_excluded_addresses(net).await;
        if let Some(address) = address {
            info!("Device excluded address last run: {}", address);
            if let Some(last_code) = &state.last_code_executed {
                database.exclude_address(address, last_code.clone(), ExcludeType::Normal);
            }

            if let Err(err) = net
                .send(OtaC2DTransport::Blacklist {
                    address: vec![address],
                })
                .await
            {
                error!("Failed to blacklist address: {:?}", err);
                return CommandExitResult::RetryOrReconnect;
            }
        }

        match state.last_reported_exclusion {
            Some((last_address, times)) if last_address == address => {
                state.last_reported_exclusion = Some((last_address, times + 1));
            }
            Some((last_address, _)) => {
                state.last_reported_exclusion = Some((last_address, 1));
            }
            None => {
                state.last_reported_exclusion = Some((address, 1));
            }
        }
    }

    if state.fsm == FSM::Uninitialized {
        // prepare for fuzzing
        // collect ground truth coverage

        state.genetic_pool = GeneticPool::new_random_population(
            GeneticPoolSettings::default(),
            state.random_source.as_mut().unwrap(),
        );

        state.fsm = FSM::Running;
    }

    if state.fsm == FSM::Running {
        while state.evolution < MAX_EVOLUTIONS {
            for (i, sample) in state.genetic_pool.all_samples_mut().iter_mut().enumerate() {
                if sample.rating.is_some() {
                    continue;
                }
                if (i % 10) == 0 {
                    let _ = database.save().await.map_err(|e| {
                        error!("Failed to save the database: {:?}", e);
                    });
                }

                if let Some((address, times)) = state.last_reported_exclusion {
                    if times > 5 {
                        error!("Device excluded the same address {address:?} more than 5 times");
                        sample.rating = Some(GeneticSampleRating::default());
                        state.last_reported_exclusion = None;
                        continue;
                    }
                }

                if state.last_code_executed.is_none() {
                    state.last_code_executed = Some(Vec::new())
                }
                if let Some(last_code) = state.last_code_executed.as_mut() {
                    last_code.clear();
                    last_code.extend(sample.code());
                }

                let (result, events) =
                    match net_execute_sample(net, interface, database, sample.code()).await {
                        ExecuteSampleResult::Timeout => return CommandExitResult::ForceReconnect,
                        ExecuteSampleResult::Rerun => return CommandExitResult::Operational,
                        ExecuteSampleResult::Success(a, b) => (a, b),
                    };

                sample.rating = Some(result.fitness.clone());
                database.push_results(
                    sample.code().to_vec(),
                    result,
                    events,
                    state.evolution,
                    state.seed,
                );
                state.last_reported_exclusion = None;
            }

            state
                .genetic_pool
                .evolution(state.random_source.as_mut().unwrap());
            state.evolution += 1;

            info!("Evolution: {}", state.evolution);

            let _ = database.save().await.map_err(|e| {
                error!("Failed to save the database: {:?}", e);
            });

            return CommandExitResult::Operational;
        }
    }

    // preparation done
    // start fuzzing
    state.fsm = FSM::Uninitialized;

    CommandExitResult::Operational
}

pub enum ExecuteSampleResult {
    Timeout,
    Rerun,
    Success(ExecutionResult, Vec<ReportExecutionProblem>),
}

