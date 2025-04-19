use crate::database::Database;
use crate::device_connection::DeviceConnection;
use crate::fuzzer_node_bridge::FuzzerNodeInterface;
use crate::net::{net_execute_sample, net_fuzzing_pretext, ExecuteSampleResult};
use crate::CommandExitResult;
use fuzzer_data::genetic_pool::{GeneticPool, GeneticPoolSettings, GeneticSampleRating};
use fuzzer_data::instruction_corpus::CorpusInstruction;
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
pub const MAX_EVOLUTIONS: u64 = 8;

pub async fn main(
    net: &mut DeviceConnection,
    interface: &FuzzerNodeInterface,
    database: &mut Database,
    state: &mut BreedingState,
    corpus: Option<&Vec<CorpusInstruction>>,
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
        // prepare for fuzzing
        // collect ground truth coverage

        match corpus {
            None => {
                state.genetic_pool = GeneticPool::new_random_population(
                    GeneticPoolSettings::default(),
                    state.random_source.as_mut().unwrap(),
                )
            }
            Some(corpus) => {
                state.genetic_pool = GeneticPool::new_random_population_from_corpus(
                    GeneticPoolSettings::default(),
                    state.random_source.as_mut().unwrap(),
                    corpus,
                );
            }
        }

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
                        ExecuteSampleResult::Success((a, b)) => (a, b),
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

    let _ = database.save().await.map_err(|e| {
        error!("Failed to save the database: {:?}", e);
    });

    // preparation done
    // start fuzzing
    state.fsm = FSM::Uninitialized;

    CommandExitResult::Operational
}
