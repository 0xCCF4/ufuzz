use fuzzer_data::{Ota, OtaC2DTransport, OtaD2CTransport, OtaD2CUnreliable};
use fuzzer_master::device_connection::{DeviceConnection, DeviceConnectionError};
use fuzzer_master::fuzzer_node_bridge::FuzzerNodeInterface;
use log::{error, info, trace, warn};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;

#[tokio::main]
async fn main() {
    env_logger::init();

    let interface = Arc::new(FuzzerNodeInterface::new("http://192.168.0.6:8000"));
    let mut udp = DeviceConnection::new("192.168.0.44:4444").await.unwrap();

    match interface.alive().await {
        Ok(x) => {
            if !x {
                eprintln!("Fuzzer node HTTP is not alive");
                std::process::exit(-1);
            }
        }
        Err(e) => {
            eprintln!("Failed to connect to the fuzzer node HTTP: {:?}", e);
            std::process::exit(-1);
        }
    }

    // guarantee initial state
    loop {
        match wait_for_device(&mut udp).await {
            WaitForDeviceResult::NoResponse => {
                power_on(&interface).await;
            }
            WaitForDeviceResult::SocketError(err) => {
                eprintln!("Failed to communicate with the device: {:?}", err);
                std::process::exit(-1);
            }
            WaitForDeviceResult::DeviceFound => {
                // device is already on
                break;
            }
        }
    }

    let mut seed = 0;

    loop {
        info!("Starting genetic fuzzing with seed {}", seed);

        udp.send(OtaC2DTransport::StartGeneticFuzzing {
            seed,
            evolutions: 1,
            random_mutation_chance: 0.01,
            code_size: 10,
            keep_best_x_solutions: 10,
            population_size: 100,
            random_solutions_each_generation: 2,
        })
        .await
        .expect("Failed to send the StartGeneticFuzzing command");

        let mut connection_lost = false;

        let mut last_iteration_count = 0;
        let mut last_iteration_time = Instant::now();

        loop {
            if last_iteration_time.elapsed() > Duration::from_secs(120) {
                error!("Device is alive, but not updating the iteration count");
            }

            match udp.receive_timeout(Duration::from_secs(60)).await {
                Some(data) => {
                    match data {
                        Ota::Unreliable(OtaD2CUnreliable::Ack(_)) => {
                            // ignore
                        }
                        Ota::Unreliable(OtaD2CUnreliable::Alive { iteration }) => {
                            info!("Iteration: {}", iteration);
                            if iteration > last_iteration_count {
                                last_iteration_count = iteration;
                                last_iteration_time = Instant::now();
                            }
                        }
                        Ota::Transport { content, .. } => match content {
                            OtaD2CTransport::FinishedGeneticFuzzing => {
                                break;
                            }
                            x => {
                                warn!("Unexpected response: {:?}", x);
                            }
                        },
                    }
                }
                None => {
                    connection_lost = true;
                    break;
                }
            }
        }

        if connection_lost {
            info!("Connection lost");
            let _ = interface.power_button_long().await;
            tokio::time::sleep(Duration::from_secs(1)).await;

            // wait for pi reboot
            tokio::time::sleep(Duration::from_secs(30)).await;
            let now = Instant::now();

            loop {
                if now.elapsed() > Duration::from_secs(120) {
                    error!("Failed to reboot the PI");
                    std::process::exit(-1);
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
            info!("Rebooted the PI");

            power_on(&interface).await;

            match wait_for_device(&mut udp).await {
                WaitForDeviceResult::NoResponse => {
                    error!("Device did not come back online");
                    std::process::exit(-1);
                }
                WaitForDeviceResult::SocketError(err) => {
                    error!("Failed to communicate with the device: {:?}", err);
                    std::process::exit(-1);
                }
                WaitForDeviceResult::DeviceFound => {
                    // device is already on
                    continue;
                }
            }
        }

        seed += 1;
    }
}

pub enum WaitForDeviceResult {
    DeviceFound,
    NoResponse,
    SocketError(DeviceConnectionError),
}

async fn power_on(interface: &FuzzerNodeInterface) {
    // device is off

    trace!("Powering on the device");
    if let Err(err) = interface.power_button_short().await {
        error!("Failed to power on the device: {:?}", err);
        std::process::exit(-1);
    }

    // device is on

    trace!("Waiting for the device to boot");
    tokio::time::sleep(Duration::from_secs(50)).await;

    // bios screen is shown

    trace!("Skipping the BIOS");
    if let Err(err) = interface.skip_bios().await {
        error!("Failed to skip the BIOS: {:?}", err);
        std::process::exit(-1);
    }

    trace!("Waiting for the device to boot UEFI");
    tokio::time::sleep(Duration::from_secs(40)).await;
}

async fn wait_for_device(net: &mut DeviceConnection) -> WaitForDeviceResult {
    info!("Waiting if the device responds to connection attempts");
    let now = std::time::Instant::now();
    let mut counter = 0;
    loop {
        counter = (counter + 1) % 4;

        if now.elapsed() > std::time::Duration::from_secs(120) {
            return WaitForDeviceResult::NoResponse;
        }

        match net.send(OtaC2DTransport::AreYouThere).await {
            Ok(_) => {
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
