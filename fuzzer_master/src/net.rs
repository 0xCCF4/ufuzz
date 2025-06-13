//! Network Communication Module
//!
//! This module provides utility functions to command a fuzzing agent via network communication

use crate::database::{Database, ExcludeType};
use crate::device_connection::DeviceConnection;
use crate::fuzzer_node_bridge::FuzzerNodeInterface;
use crate::genetic_breeding::SAMPLE_TIMEOUT;
use crate::manual_execution::disassemble_code;
use crate::{wait_for_device, CommandExitResult, WaitForDeviceResult};
use fuzzer_data::{
    Code, ExecutionResult, Ota, OtaC2DTransport, OtaD2CTransport, ReportExecutionProblem,
    SpeculationResult,
};
use hypervisor::state::VmExitReason;
use itertools::Itertools;
use log::{error, info, trace, warn};
use performance_timing::measurements::MeasureValues;
use rand::random;
use std::collections::BTreeMap;
use std::time::Duration;
use ucode_compiler_dynamic::instruction::Instruction;
use ucode_compiler_dynamic::sequence_word::SequenceWord;
use x86_perf_counter::PerfEventSpecifier;

/// Sends blacklisted addresses to the device
///
/// # Arguments
///
/// * `net` - Network connection to the device
/// * `database` - Database containing blacklisted addresses
///
/// # Returns
///
/// * `bool` indicating success or failure
pub async fn net_blacklist(net: &mut DeviceConnection, database: &mut Database) -> bool {
    let vector = database.blacklisted().collect_vec();
    for blacklist in vector
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
                address: blacklist.to_vec(),
            })
            .await
        {
            error!("Failed to blacklist addresses: {:?}", err);
            return false;
        }
    }
    true
}

/// Executes a code sample on the device
///
/// # Arguments
///
/// * `net` - Network connection to the device
/// * `interface` - Interface to the fuzzing node
/// * `db` - Database for storing results
/// * `sample` - Code sample to execute
///
/// # Returns
///
/// * `ExecuteSampleResult` containing execution results and events
pub async fn net_execute_sample(
    net: &mut DeviceConnection,
    interface: &FuzzerNodeInterface,
    db: &mut Database,
    sample: &[u8],
) -> ExecuteSampleResult<(ExecutionResult, Vec<ReportExecutionProblem>)> {
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

    ExecuteSampleResult::Success((result, events))
}

/// Receives excluded addresses from the device (if the agent did exclude an address last run)
///
/// # Arguments
///
/// * `net` - Network connection to the device
///
/// # Returns
///
/// * `Option<u16>` containing the excluded address if any
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

/// Reboots the fuzzing device
///
/// # Arguments
///
/// * `net` - Network connection to the device
/// * `interface` - Interface to the fuzzing node
///
/// # Returns
///
/// * `Option<CommandExitResult>` indicating the outcome of the reboot
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

/// Receives execution results from the device
///
/// # Arguments
///
/// * `net` - Network connection to the device
/// * `timeout` - Timeout duration for receiving results
///
/// # Returns
///
/// * `Option<(ExecutionResult, Vec<ReportExecutionProblem>)>` containing execution results and events
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

/// Receives performance timing data from the device
///
/// # Arguments
///
/// * `net` - Network connection to the device
/// * `timeout` - Timeout duration for receiving data
///
/// # Returns
///
/// * `Option<BTreeMap<String, MeasureValues<f64>>>` containing performance measurements
pub async fn net_receive_performance_timing(
    net: &mut DeviceConnection,
    timeout: Duration,
) -> Option<BTreeMap<String, MeasureValues<f64>>> {
    let mut data = BTreeMap::default();

    let _ = net
        .send(OtaC2DTransport::ReportPerformanceTiming)
        .await
        .map_err(|e| {
            error!("Failed to send ReportPerformanceTiming: {:?}", e);
        });

    loop {
        let packet = net.receive(Some(timeout)).await;

        if packet.is_none() && data.is_empty() {
            return None;
        }

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
            return Some(data);
        }
    }
}

/// Results of executing a code sample
#[derive(Debug)]
pub enum ExecuteSampleResult<T> {
    /// Execution timed out
    Timeout,
    /// Sample needs to be rerun
    Rerun,
    /// Execution completed successfully
    Success(T),
}

/// Performs pre-execution setup for fuzzing
///
/// # Arguments
///
/// * `net` - Network connection to the device
/// * `database` - Database for storing results
/// * `last_code_executed` - Last code sample that was executed
/// * `last_reported_exclusion` - Last reported address exclusion
///
/// # Returns
///
/// * `CommandExitResult` indicating the outcome of setup
pub async fn net_fuzzing_pretext(
    net: &mut DeviceConnection,
    database: &mut Database,
    last_code_executed: &mut Option<Code>,
    last_reported_exclusion: &mut Option<(Option<u16>, u16)>,
) -> CommandExitResult {
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
            if let Some(last_code) = last_code_executed {
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

        match last_reported_exclusion {
            Some((last_address, times)) if *last_address == address => {
                *last_reported_exclusion = Some((*last_address, *times + 1));
            }
            Some((last_address, _)) => {
                *last_reported_exclusion = Some((*last_address, 1));
            }
            None => {
                *last_reported_exclusion = Some((address, 1));
            }
        }

        CommandExitResult::Operational
    }
}

/// Receives speculative execution results from the device
///
/// # Arguments
///
/// * `net` - Network connection to the device
/// * `timeout` - Timeout duration for receiving results
///
/// # Returns
///
/// * `Option<SpeculationResult>` containing speculative execution results
pub async fn net_receive_speculative_result(
    net: &mut DeviceConnection,
    timeout: Duration,
) -> Option<SpeculationResult> {
    loop {
        let packet = net.receive(Some(timeout)).await;

        if let Some(packet) = packet {
            if let Ota::Transport { content, .. } = packet {
                match content {
                    OtaD2CTransport::UCodeSpeculationResult(result) => {
                        return Some(result);
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

/// Executes a speculative sample on the device
///
/// # Arguments
///
/// * `net` - Network connection to the device
/// * `triad` - Array of three instructions to execute
/// * `sequence_word` - Sequence word for execution
/// * `perf_counter_setup` - Performance counter configuration
///
/// # Returns
///
/// * `ExecuteSampleResult<SpeculationResult>` containing speculative execution results
pub async fn net_speculative_sample(
    net: &mut DeviceConnection,
    triad: [Instruction; 3],
    sequence_word: SequenceWord,
    perf_counter_setup: Vec<PerfEventSpecifier>,
) -> ExecuteSampleResult<SpeculationResult> {
    if let Err(err) = net
        .send(OtaC2DTransport::UCodeSpeculation {
            triad,
            sequence_word,
            perf_counter_setup,
        })
        .await
    {
        error!("Failed to execute sample: {:?}", err);
        return ExecuteSampleResult::Timeout;
    }

    ExecuteSampleResult::Success(
        match net_receive_speculative_result(net, Duration::from_secs(10)).await {
            None => return ExecuteSampleResult::Timeout,
            Some(x) => x,
        },
    )
}
