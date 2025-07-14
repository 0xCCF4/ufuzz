//! Fuzzer Master Library
//!
//! This library implements the master control logic for coordinating fuzzing the fuzzing operations.

use crate::device_connection::{DeviceConnection, DeviceConnectionError};
use crate::fuzzer_node_bridge::FuzzerNodeInterface;
use fuzzer_data::OtaC2DTransport;
use log::{debug, error, info, trace, warn};
use performance_timing::track_time;
use std::time::Duration;
use tokio::time::Instant;

pub mod database;
pub mod device_connection;
pub mod fuzzer_node_bridge;

pub mod genetic_breeding;

pub mod afl_fuzzing;
pub mod instruction_mutations;

pub mod manual_execution;
pub mod net;

pub mod spec_fuzz;

/// Results of command execution that determine program flow
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CommandExitResult {
    /// Program should exit
    ExitProgram,
    /// Command should be retried or connection reestablished
    RetryOrReconnect,
    /// Connection must be forcibly reestablished
    ForceReconnect,
    /// Command completed successfully
    Operational,
}

/// Base frequency for x86_64 development machine
#[cfg(target_arch = "x86_64")]
pub const P0_FREQ: f64 = 2_699_000_000.0; // Our development machine

/// Base frequency for Raspberry Pi 4
#[cfg(target_arch = "aarch64")]
pub const P0_FREQ: f64 = 54_000_000.0; // Rpi4

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
compiler_error!("Unsupported architecture");

/// Waits for a Raspberry Pi to complete its reboot sequence
///
/// # Arguments
///
/// * `interface` - Interface to the fuzzing node
///
/// # Panics
///
/// Panics if the Pi fails to respond within 5 min
pub async fn wait_for_pi(interface: &FuzzerNodeInterface) {
    trace!("Waiting for the PI to reboot");
    let now = Instant::now();
    loop {
        if now.elapsed() > Duration::from_secs(300) {
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

/// Results of waiting for a device to respond
pub enum WaitForDeviceResult {
    /// Device was found and responded
    DeviceFound,
    /// No response received from device
    NoResponse,
    /// Socket error occurred during communication
    SocketError(DeviceConnectionError),
}

/// Waits for the fuzzing agent to respond to connection attempts
///
/// # Arguments
///
/// * `net` - Network connection to the device
///
/// # Returns
///
/// * `WaitForDeviceResult` indicating the outcome of the wait operation
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

/// Powers on the fuzzing agent and handles its boot sequence
///
/// # Arguments
///
/// * `interface` - Interface to the fuzzing node
///
/// # Returns
///
/// * `bool` indicating whether the power-on sequence was successful
#[track_time("host::guarantee_state")]
pub async fn power_on(interface: &FuzzerNodeInterface) -> bool {
    // device is off

    trace!("Powering on the device");
    if let Err(err) = interface.power_button_short().await {
        error!("Failed to power on the device: {:?}", err);
        if let Err(err) = interface.power_button_short().await {
            error!("Failed to power on the device: {:?}", err);
            return false;
        }
    }

    // device is on

    trace!("Waiting for the device to boot");
    tokio::time::sleep(Duration::from_secs(50)).await;

    // bios screen is shown

    trace!("Skipping the BIOS");
    if let Err(err) = interface.skip_bios().await {
        error!("Failed to skip the BIOS: {:?}", err);
        return false;
    }

    trace!("Waiting for the device to boot UEFI");
    tokio::time::sleep(Duration::from_secs(40)).await;

    true
}

/// Ensures that the fuzzing agent is in a known initial state
///
/// This function handles device power management and connection establishment,
/// retrying as necessary until the device is in a usable state.
///
/// # Arguments
///
/// * `interface` - Interface to the fuzzing node
/// * `udp` - Network connection to the device
#[track_time("host::guarantee_state")]
pub async fn guarantee_initial_state(interface: &FuzzerNodeInterface, udp: &mut DeviceConnection) {
    // guarantee initial state
    let mut iteration = 0;
    loop {
        match wait_for_device(udp).await {
            WaitForDeviceResult::NoResponse => {
                power_on(&interface).await;
                iteration += 1;

                if (iteration % 2) == 0 {
                    warn!("Device did not boot after 2 attempts, trying a long press");
                    let _ = interface.power_button_long().await;
                }
            }
            WaitForDeviceResult::SocketError(err) => {
                eprintln!("Failed to communicate with the device: {:?}", err);
                continue;
            }
            WaitForDeviceResult::DeviceFound => {
                // device is already on
                break;
            }
        }
    }
}
