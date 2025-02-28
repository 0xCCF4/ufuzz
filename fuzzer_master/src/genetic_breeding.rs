use crate::database::Database;
use crate::device_connection::DeviceConnection;
use crate::fuzzer_node_bridge::FuzzerNodeInterface;
use crate::{wait_for_device, CommandExitResult};
use fuzzer_data::genetic_pool::{GeneticPool, GeneticPoolSettings, GeneticSampleRating};
use fuzzer_data::{ExecutionResult, Ota, OtaC2DTransport, OtaD2CTransport, ReportExecutionProblem};
use hypervisor::state::VmExitReason;
use itertools::Itertools;
use log::{error, info, trace, warn};
use rand::{random, SeedableRng};
use std::collections::BTreeMap;
use std::time::Duration;

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

    for blacklist in &database
        .data
        .blacklisted_addresses
        .iter()
        .chain(database.data.vm_entry_blacklist.iter())
        .chunks(200)
    {
        if let Err(err) = net
            .send(OtaC2DTransport::Blacklist {
                address: blacklist.cloned().collect_vec(),
            })
            .await
        {
            error!("Failed to blacklist addresses: {:?}", err);
            return CommandExitResult::RetryOrReconnect;
        }
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
            database.data.blacklisted_addresses.insert(address);

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

        let (mut result, events) = match net_execute_sample(net, interface, database, &[]).await {
            ExecuteSampleResult::Timeout => return CommandExitResult::RetryOrReconnect,
            ExecuteSampleResult::Rerun => return CommandExitResult::Operational,
            ExecuteSampleResult::Success(a, b) => (a, b),
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

                let (mut result, events) =
                    match net_execute_sample(net, interface, database, sample.code()).await {
                        ExecuteSampleResult::Timeout => return CommandExitResult::ForceReconnect,
                        ExecuteSampleResult::Rerun => return CommandExitResult::Operational,
                        ExecuteSampleResult::Success(a, b) => (a, b),
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

pub fn subtract_iter_btree<'a, 'b>(
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

pub enum ExecuteSampleResult {
    Timeout,
    Rerun,
    Success(ExecutionResult, Vec<ReportExecutionProblem>),
}

pub async fn net_execute_sample(
    net: &mut DeviceConnection,
    interface: &FuzzerNodeInterface,
    db: &mut Database,
    sample: &[u8],
) -> ExecuteSampleResult {
    if let Err(err) = net
        .send(OtaC2DTransport::ExecuteSample {
            code: sample.to_vec(),
        })
        .await
    {
        error!("Failed to execute sample: {:?}", err);
        return ExecuteSampleResult::Timeout;
    }

    let (result, events) =
        match net_receive_execution_result(net, Duration::from_secs(SAMPLE_TIMEOUT)).await {
            None => return ExecuteSampleResult::Timeout,
            Some(x) => x,
        };

    let cov_mismatch = events
        .iter()
        .filter_map(|x| {
            if let ReportExecutionProblem::CoverageProblem {
                address,
                coverage_exit,
                ..
            } = x
            {
                if let Some(VmExitReason::VMEntryFailure(_, _)) = coverage_exit {
                    Some(*address)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .sorted()
        .next();

    if let Some(first) = cov_mismatch {
        info!("VM entry failure during coverage collection: {:#x}", first);
        db.data.vm_entry_blacklist.insert(first);
        db.data.vm_entry_blacklist.insert(first - 2);
        db.mark_dirty();

        trace!("Rebooting device...");
        let _ = net.send(OtaC2DTransport::Reboot).await;
        tokio::time::sleep(Duration::from_secs(60)).await;
        trace!("Skipping BIOS...");
        let _ = interface.skip_bios().await;
        tokio::time::sleep(Duration::from_secs(100)).await;
        let _ = wait_for_device(net).await;
        return ExecuteSampleResult::Rerun;
    }

    ExecuteSampleResult::Success(result, events)
}

pub async fn net_receive_excluded_addresses(net: &mut DeviceConnection) -> Option<u16> {
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

pub async fn net_receive_execution_result(
    net: &mut DeviceConnection,
    timeout: Duration,
) -> Option<(ExecutionResult, Vec<ReportExecutionProblem>)> {
    let mut events = Vec::new();
    let mut serialized_code = None;
    let mut coverage_result = BTreeMap::new();

    loop {
        let packet = net.receive(Some(timeout)).await;

        if let Some(packet) = packet {
            if let Ota::Transport { content, .. } = packet {
                match content {
                    OtaD2CTransport::ExecutionEvents(new_events) => events.extend(new_events),
                    OtaD2CTransport::Serialized { serialized } => {
                        serialized_code = serialized;
                    }
                    OtaD2CTransport::Coverage { coverage } => {
                        for cov in coverage {
                            coverage_result.insert(cov.0, cov.1);
                        }
                    }
                    OtaD2CTransport::ExecutionResult {
                        exit,
                        state,
                        fitness,
                    } => {
                        return Some((
                            ExecutionResult {
                                coverage: coverage_result,
                                exit,
                                state,
                                serialized: serialized_code,
                                fitness,
                            },
                            events,
                        ));
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
