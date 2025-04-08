use crate::database::{Database, ExcludeType};
use crate::device_connection::DeviceConnection;
use crate::fuzzer_node_bridge::FuzzerNodeInterface;
use crate::manual_execution::disassemble_code;
use crate::{wait_for_device, CommandExitResult, WaitForDeviceResult};
use fuzzer_data::genetic_pool::{GeneticPool, GeneticPoolSettings, GeneticSampleRating};
use fuzzer_data::{
    Code, ExecutionResult, Ota, OtaC2DTransport, OtaD2CTransport, ReportExecutionProblem,
};
use hypervisor::state::VmExitReason;
use itertools::Itertools;
use log::{error, info, trace, warn};
use performance_timing::measurements::MeasureValues;
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

pub async fn net_blacklist(net: &mut DeviceConnection, database: &mut Database) -> bool {
    for blacklist in &database
        .blacklisted()
        /*.chain(
            database
                .data
                .results
                .iter()
                .map(|r| r.events.iter())
                .flatten()
                .filter_map(|e| {
                    if let CodeEvent::CoverageProblem { address, .. } = e {
                        Some(address)
                    } else {
                        None
                    }
                })
                .unique(),
        )*/
        .chunks(200)
    {
        if let Err(err) = net
            .send(OtaC2DTransport::Blacklist {
                address: blacklist.collect_vec(),
            })
            .await
        {
            error!("Failed to blacklist addresses: {:?}", err);
            return false;
        }
    }
    true
}

pub async fn net_execute_sample(
    net: &mut DeviceConnection,
    interface: &FuzzerNodeInterface,
    db: &mut Database,
    sample: &[u8],
) -> ExecuteSampleResult {
    let mut disasm = String::new();
    disassemble_code(sample, &mut disasm);
    trace!("{}", disasm);

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
        db.exclude_address(first, sample.to_vec(), ExcludeType::VMentry);
        db.exclude_address(first - 2, sample.to_vec(), ExcludeType::VMentry);
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

    if result.coverage.len() == 0 {
        warn!("Empty coverage.");
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

pub async fn net_reboot_device(
    net: &mut DeviceConnection,
    interface: &FuzzerNodeInterface,
) -> Option<CommandExitResult> {
    let x = net.send(OtaC2DTransport::Reboot).await;
    if let Err(_) = x {
        Some(CommandExitResult::RetryOrReconnect)
    } else {
        tokio::time::sleep(Duration::from_secs(60)).await;
        trace!("Skipping bios");
        let _ = interface.skip_bios().await;
        tokio::time::sleep(Duration::from_secs(40)).await;
        let x = wait_for_device(net).await;
        match x {
            WaitForDeviceResult::DeviceFound => None,
            _ => Some(CommandExitResult::RetryOrReconnect),
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
                            //println!("COV: {:x?}", cov);
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

pub async fn net_receive_performance_timing(
    net: &mut DeviceConnection,
    timeout: Duration,
) -> BTreeMap<String, MeasureValues<f64>> {
    let mut data = BTreeMap::default();

    let _ = net
        .send(OtaC2DTransport::ReportPerformanceTiming)
        .await
        .map_err(|e| {
            error!("Failed to send ReportPerformanceTiming: {:?}", e);
        });

    loop {
        let packet = net.receive(Some(timeout)).await;

        if let Some(packet) = packet {
            if let Ota::Transport { content, .. } = packet {
                match content {
                    OtaD2CTransport::PerformanceTiming { measurements } => {
                        for (k, v) in measurements {
                            if data.insert(k.to_string(), v).is_some() {
                                error!("Performance timing key collision: {k:?}");
                            }
                        }
                    }
                    _ => {
                        warn!("Unexpected packet: {:?}", content);
                    }
                }
            }
        } else {
            return data;
        }
    }
}
