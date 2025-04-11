use crate::device_connection::{DeviceConnection, DeviceConnectionError};
use crate::fuzzer_node_bridge::FuzzerNodeInterface;
use fuzzer_data::OtaC2DTransport;
use log::{debug, error, info, trace};
use std::time::Duration;
use tokio::time::Instant;

pub mod database;
pub mod device_connection;
pub mod fuzzer_node_bridge;

pub mod genetic_breeding;
pub mod instruction_mutations;

pub mod manual_execution;
pub mod net;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CommandExitResult {
    ExitProgram,
    RetryOrReconnect,
    ForceReconnect,
    Operational,
}

#[cfg(target_arch = "x86_64")]
pub const P0_FREQ: f64 = 2_699_000_000.0; // Our development machine

#[cfg(target_arch = "aarch64")]
pub const P0_FREQ: f64 = 54_000_000.0; // Rpi4

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
compiler_error!("Unsupported architecture");

pub async fn wait_for_pi(interface: &FuzzerNodeInterface) {
    trace!("Waiting for the PI to reboot");
    let now = Instant::now();
    loop {
        if now.elapsed() > Duration::from_secs(120) {
            error!("Failed to reboot the PI");
            std::process::exit(-1); // we cant do anything since the PI is not responding
        }

        if let Ok(true) = interface.alive().await {
            break;
        }

        match interface.alive().await {
            Ok(true) => {
                break;
            }
            x => {
                trace!("HTTP not ready: {:?}", x);
            }
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    trace!("PI is ready");
}

pub enum WaitForDeviceResult {
    DeviceFound,
    NoResponse,
    SocketError(DeviceConnectionError),
}

pub async fn wait_for_device(net: &mut DeviceConnection) -> WaitForDeviceResult {
    info!("Waiting if the device responds to connection attempts");
    let now = std::time::Instant::now();
    let mut counter = 0;
    loop {
        counter = (counter + 1) % 4;

        if now.elapsed() > std::time::Duration::from_secs(120) {
            debug!("No");
            return WaitForDeviceResult::NoResponse;
        }

        match net.send(OtaC2DTransport::AreYouThere).await {
            Ok(_) => {
                info!("Yes");
                return WaitForDeviceResult::DeviceFound;
            }
            Err(DeviceConnectionError::NoAckReceived) => {
                // no response
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
            Err(e) => {
                return WaitForDeviceResult::SocketError(e);
            }
        }
    }
}
