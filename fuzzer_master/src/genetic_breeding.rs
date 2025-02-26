use crate::database::Database;
use crate::device_connection::DeviceConnection;
use fuzzer_data::genetic_pool::{GeneticPool, GeneticPoolSettings, GeneticSampleRating};
use fuzzer_data::{ExecutionResult, Ota, OtaC2DTransport, OtaD2CTransport, ReportExecutionProblem};
use itertools::Itertools;
use log::{error, info, warn};
use rand::{random, SeedableRng};
use std::collections::BTreeMap;
use std::time::Duration;

pub enum GeneticBreedingResult {
    ConnectionLost,
    Reconnect,
    Finished,
    Continue,
}

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
    ground_truth_coverage: BTreeMap<u16, u16>,

    last_reported_exclusion: Option<(Option<u16>, u16)>, // address, times

    evolution: u64,
    seed: u64,
}

pub const SAMPLE_TIMEOUT: u64 = 60;
pub const MAX_EVOLUTIONS: u64 = 100;

pub async fn main(
    net: &mut DeviceConnection,
    database: &mut Database,
    state: &mut BreedingState,
) -> GeneticBreedingResult {
    // device is either restarted or new experimentation run

    match state.fsm {
        FSM::Uninitialized => {
            // new experimentation run
            let seed = random();

            info!("Starting new experimentation run with seed: {}", seed);
            state.random_source = Some(rand_isaac::Isaac64Rng::seed_from_u64(seed));
            state.seed = seed;
            state.evolution = 1;
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
        return GeneticBreedingResult::Reconnect;
    }

    for blacklist in &database.blacklisted_addresses.iter().chunks(200) {
        if let Err(err) = net
            .send(OtaC2DTransport::Blacklist {
                address: blacklist.cloned().collect_vec(),
            })
            .await
        {
            error!("Failed to blacklist addresses: {:?}", err);
            return GeneticBreedingResult::Reconnect;
        }
    }

    if state.fsm == FSM::Uninitialized {
        // prepare for fuzzing
        // collect ground truth coverage
        if let Err(err) = net
            .send(OtaC2DTransport::ExecuteSample { code: vec![] })
            .await
        {
            error!("Failed to execute sample: {:?}", err);
            return GeneticBreedingResult::Reconnect;
        }

        let (mut result, events) =
            match net_receive_execution_result(net, Duration::from_secs(SAMPLE_TIMEOUT)).await {
                None => return GeneticBreedingResult::Reconnect,
                Some(x) => x,
            };

        state.ground_truth_coverage.clear();
        for (address, coverage) in &result.coverage {
            state.ground_truth_coverage.insert(*address, *coverage);
        }
        result.coverage.clear();

        database.push_results(vec![], result, events, 0, 0);

        state.genetic_pool = GeneticPool::new_random_population(
            GeneticPoolSettings::default(),
            state.random_source.as_mut().unwrap(),
        );

        state.fsm = FSM::Running;
    }

    if let Err(err) = net
        .send(OtaC2DTransport::DidYouExcludeAnAddressLastRun)
        .await
    {
        error!(
            "Failed to ask device if it excluded an address last run: {:?}",
            err
        );
        return GeneticBreedingResult::Reconnect;
    } else {
        let address = net_receive_excluded_addresses(net).await;
        if let Some(address) = address {
            info!("Device excluded address last run: {}", address);
            database.blacklisted_addresses.insert(address);

            if let Err(err) = net
                .send(OtaC2DTransport::Blacklist {
                    address: vec![address],
                })
                .await
            {
                error!("Failed to blacklist address: {:?}", err);
                return GeneticBreedingResult::Reconnect;
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

    if state.fsm == FSM::Running {
        while state.evolution < MAX_EVOLUTIONS {
            for sample in state.genetic_pool.all_samples_mut() {
                if sample.rating.is_some() {
                    continue;
                }

                if let Some((address, times)) = state.last_reported_exclusion {
                    if times > 5 {
                        error!("Device excluded the same address {address:?} more than 5 times");
                        sample.rating = Some(GeneticSampleRating::default());
                        state.last_reported_exclusion = None;
                        continue;
                    }
                }

                if let Err(err) = net
                    .send(OtaC2DTransport::ExecuteSample {
                        code: sample.code().to_vec(),
                    })
                    .await
                {
                    error!("Failed to execute sample: {:?}", err);
                    return GeneticBreedingResult::Reconnect;
                }

                let (mut result, events) =
                    match net_receive_execution_result(net, Duration::from_secs(SAMPLE_TIMEOUT))
                        .await
                    {
                        None => return GeneticBreedingResult::Reconnect,
                        Some(x) => x,
                    };

                subtract_iter_btree(&mut result.coverage, &state.ground_truth_coverage);
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

            return GeneticBreedingResult::Continue;
        }
    }

    // preparation done
    // start fuzzing
    state.fsm = FSM::Uninitialized;

    GeneticBreedingResult::Finished
}

fn subtract_iter_btree<'a, 'b>(
    original: &'a mut BTreeMap<u16, u16>,
    subtract_this: &'b BTreeMap<u16, u16>,
) {
    original.retain(|k, v| {
        if let Some(subtract_v) = subtract_this.get(k) {
            if *v < *subtract_v {
                error!(
                    "Reducing coverage count for more than ground truth: {k} : {v} < {subtract_v}"
                );
            }
            *v = v.saturating_sub(*subtract_v);
        }
        *v != 0
    });
}

async fn net_receive_excluded_addresses(net: &mut DeviceConnection) -> Option<u16> {
    let result = net
        .receive_packet(
            |p| {
                matches!(
                    p,
                    Ota::Transport {
                        content: OtaD2CTransport::LastRunBlacklisted { .. },
                        ..
                    }
                )
            },
            Some(Duration::from_secs(3)),
        )
        .await;

    match result {
        Ok(Some(Ota::Transport {
            content: OtaD2CTransport::LastRunBlacklisted { address, .. },
            ..
        })) => address,
        Ok(Some(x)) => {
            warn!("Unexpected packet: {:?}", x);
            None
        }
        Ok(None) => None,
        Err(e) => {
            if !e.is_timeout() {
                error!("Failed to receive excluded addresses: {:?}", e);
            }
            None
        }
    }
}

async fn net_receive_execution_result(
    net: &mut DeviceConnection,
    timeout: Duration,
) -> Option<(ExecutionResult, Vec<ReportExecutionProblem>)> {
    let mut events = Vec::new();

    loop {
        let packet = net.receive(Some(timeout)).await;

        if let Some(packet) = packet {
            if let Ota::Transport { content, .. } = packet {
                match content {
                    OtaD2CTransport::ExecutionEvents(new_events) => events.extend(new_events),
                    OtaD2CTransport::ExecutionResult(result) => {
                        return Some((result, events));
                    }
                    _ => {
                        warn!("Unexpected packet: {:?}", content);
                    }
                }
            }
        } else {
            return None;
        }
    }
}
