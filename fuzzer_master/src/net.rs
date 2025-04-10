use std::collections::BTreeMap;
use std::time::Duration;
use itertools::Itertools;
use log::{error, info, trace, warn};
use fuzzer_data::{ExecutionResult, Ota, OtaC2DTransport, OtaD2CTransport, ReportExecutionProblem};
use hypervisor::state::VmExitReason;
use performance_timing::measurements::MeasureValues;
use crate::database::{Database, ExcludeType};
use crate::device_connection::DeviceConnection;
use crate::fuzzer_node_bridge::FuzzerNodeInterface;
use crate::genetic_breeding::{SAMPLE_TIMEOUT};
use crate::manual_execution::disassemble_code;
use crate::{wait_for_device, CommandExitResult, WaitForDeviceResult};

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

pub enum ExecuteSampleResult {
    Timeout,
    Rerun,
    Success(ExecutionResult, Vec<ReportExecutionProblem>),
}
