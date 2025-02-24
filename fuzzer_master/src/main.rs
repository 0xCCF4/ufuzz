use fuzzer_data::{Ota, OtaC2DTransport, OtaD2C, OtaD2CTransport, OtaD2CUnreliable};
use fuzzer_master::database::Database;
use fuzzer_master::device_connection::{DeviceConnection, DeviceConnectionError};
use fuzzer_master::fuzzer_node_bridge::FuzzerNodeInterface;
use itertools::Itertools;
use log::{error, info, trace, warn};
use std::io;
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;

pub const DATABASE_FILE: &str = "database.json";

#[tokio::main]
async fn main() {
    env_logger::init();

    let mut database = fuzzer_master::database::Database::from_file(DATABASE_FILE).map_or_else(
        |e| {
            println!("Failed to load the database: {:?}", e);
            print!("Do you want to create a new one? (y/n): ");
            io::stdout().flush().unwrap();

            let mut input = String::new();
            io::stdin().read_line(&mut input).unwrap();

            if input.trim().eq_ignore_ascii_case("y") {
                Database::default()
            } else {
                std::process::exit(1);
            }
        },
        |x| x,
    );
    let interface = Arc::new(FuzzerNodeInterface::new("http://192.168.0.10:8000"));
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

    guarantee_initial_state(&interface, &mut udp).await;

    // get blacklisted
    let _ = udp
        .send(OtaC2DTransport::GiveMeYourBlacklistedAddresses)
        .await
        .map_err(|e| {
            error!("Failed to ask for blacklisted addresses: {:?}", e);
        });

    while let Ok(Some(Ota::Transport {
        content: OtaD2CTransport::BlacklistedAddresses { addresses },
        ..
    })) = udp
        .receive_packet_timeout(
            |p| {
                matches!(
                    p,
                    OtaD2C::Transport {
                        content: OtaD2CTransport::BlacklistedAddresses { .. },
                        ..
                    }
                )
            },
            Duration::from_secs(3),
        )
        .await
    {
        database.blacklisted_addresses.extend(addresses);
    }
    let _ = database.save(DATABASE_FILE).map_err(|e| {
        error!("Failed to save the database: {:?}", e);
    });

    let mut seed = 0;
    let mut last_seed_iteration_last_time = None;
    let mut last_seed_iteration = 0;

    loop {
        info!("Starting genetic fuzzing with seed {}", seed);

        udp.flush_read_timeout(Duration::from_secs(1)).await;

        let _ = udp
            .send(OtaC2DTransport::DidYouExcludeAnAddressLastRun)
            .await
            .map_err(|e| {
                error!(
                    "Failed to ask if the device excluded an address last run: {:?}",
                    e
                );
            });

        let answer = udp
            .receive_packet_timeout(
                |p| {
                    matches!(
                        p,
                        OtaD2C::Transport {
                            content: OtaD2CTransport::LastRunBlacklisted { .. },
                            ..
                        }
                    )
                },
                Duration::from_secs(3),
            )
            .await;
        if let Ok(Some(Ota::Transport {
            content: OtaD2CTransport::LastRunBlacklisted { address },
            ..
        })) = answer
        {
            if let Some(address) = address {
                info!("Device excluded address: {:04x}", address);
                database.blacklisted_addresses.insert(address);
                let _ = database.save(DATABASE_FILE).map_err(|e| {
                    error!("Failed to save the database: {:?}", e);
                });
            }
        } else {
            error!(
                "Failed to ask if the device excluded an address last run: {:?}",
                answer
            );
        }

        for blacklisted_addresses in &database
            .blacklisted_addresses
            .iter()
            .chain(database.get_blacklisted_because_coverage_problem().iter())
            .chunks(60)
        {
            let _ = udp
                .send(OtaC2DTransport::Blacklist {
                    address: blacklisted_addresses.cloned().collect_vec(),
                })
                .await
                .map_err(|e| {
                    error!("Failed to blacklist addresses: {:?}", e);
                });
        }

        let _ = udp
            .send(OtaC2DTransport::StartGeneticFuzzing {
                seed,
                evolutions: 1,
                random_mutation_chance: 0.01,
                code_size: 10,
                keep_best_x_solutions: 10,
                population_size: 100,
                random_solutions_each_generation: 2,
            })
            .await
            .map_err(|e| {
                error!("Failed to start genetic fuzzing: {:?}", e);
            });

        let mut connection_lost = false;

        let mut last_iteration_count = 0;
        let mut last_iteration_time = Instant::now();

        loop {
            if last_iteration_time.elapsed() > Duration::from_secs(20) {
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
                            if iteration > last_seed_iteration {
                                last_seed_iteration = iteration;
                            }
                        }
                        Ota::Transport { content, .. } => match content {
                            OtaD2CTransport::FinishedGeneticFuzzing => {
                                break;
                            }
                            OtaD2CTransport::ExecutionEvent(event) => {
                                database.push_event(event);

                                let _ = database.save(DATABASE_FILE).map_err(|e| {
                                    error!("Failed to save the database: {:?}", e);
                                });
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
            seed += 1; // todo remove

            info!("Connection lost");
            let now = Instant::now();
            let _ = interface.power_button_long().await;

            match &mut last_seed_iteration_last_time {
                None => {
                    last_seed_iteration_last_time = Some(last_seed_iteration);
                    last_seed_iteration = 0;
                }
                Some(last_seed_iteration_last_time) => {
                    if *last_seed_iteration_last_time == last_seed_iteration {
                        // we made no progress
                        error!("Failed to make progress.");
                    }
                    *last_seed_iteration_last_time = last_seed_iteration;
                    last_seed_iteration = 0;
                }
            }

            // wait for pi reboot

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
            info!("Rebooted the PI");

            tokio::time::sleep(Duration::from_secs(3)).await;
            let _ = power_on(&interface).await;

            guarantee_initial_state(&interface, &mut udp).await;
        }

        seed += 1;
        last_seed_iteration = 0;
        last_seed_iteration_last_time = None;
    }
}

async fn guarantee_initial_state(interface: &Arc<FuzzerNodeInterface>, udp: &mut DeviceConnection) {
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

pub enum WaitForDeviceResult {
    DeviceFound,
    NoResponse,
    SocketError(DeviceConnectionError),
}

async fn power_on(interface: &FuzzerNodeInterface) -> bool {
    // device is off

    trace!("Powering on the device");
    if let Err(err) = interface.power_button_short().await {
        error!("Failed to power on the device: {:?}", err);
        return false;
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
