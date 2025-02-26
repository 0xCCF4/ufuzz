use fuzzer_data::{Ota, OtaC2DTransport, OtaD2C, OtaD2CTransport};
use fuzzer_master::database::Database;
use fuzzer_master::device_connection::{DeviceConnection, DeviceConnectionError};
use fuzzer_master::fuzzer_node_bridge::FuzzerNodeInterface;
use fuzzer_master::genetic_breeding;
use fuzzer_master::genetic_breeding::{BreedingState, GeneticBreedingResult};
use log::{debug, error, info, trace, warn};
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
            if e.kind() == std::io::ErrorKind::NotFound {
                return Database::default();
            }
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
        .receive_packet(
            |p| {
                matches!(
                    p,
                    OtaD2C::Transport {
                        content: OtaD2CTransport::BlacklistedAddresses { .. },
                        ..
                    }
                )
            },
            Some(Duration::from_secs(3)),
        )
        .await
    {
        database.blacklisted_addresses.extend(addresses);
    }
    let _ = database.save(DATABASE_FILE).map_err(|e| {
        error!("Failed to save the database: {:?}", e);
    });

    let mut state = BreedingState::default();

    let mut continue_count = 0;

    loop {
        let result = genetic_breeding::main(&mut udp, &mut database, &mut state).await;

        let connection_lost = match result {
            GeneticBreedingResult::ConnectionLost => {
                continue_count = 0;
                GeneticBreedingResult::ConnectionLost
            }
            GeneticBreedingResult::Reconnect => {
                continue_count += 1;
                if continue_count > 3 {
                    GeneticBreedingResult::ConnectionLost
                } else {
                    GeneticBreedingResult::Reconnect
                }
            }
            GeneticBreedingResult::Finished => {
                continue_count = 0;
                GeneticBreedingResult::Finished
            }
            GeneticBreedingResult::Continue => {
                continue_count = 0;
                GeneticBreedingResult::Continue
            }
        };

        let db_clone = database.clone();
        std::thread::spawn(move || {
            let _ = db_clone.save(DATABASE_FILE).map_err(|e| {
                error!("Failed to save the database: {:?}", e);
            });
        });

        if let GeneticBreedingResult::ConnectionLost = connection_lost {
            info!("Connection lost");

            let _ = interface.power_button_long().await;

            // wait for pi reboot

            wait_for_pi(&interface).await;
            tokio::time::sleep(Duration::from_secs(5)).await;

            let _ = power_on(&interface).await;

            wait_for_pi(&interface).await;
            tokio::time::sleep(Duration::from_secs(5)).await;

            guarantee_initial_state(&interface, &mut udp).await;
        }
    }
}

async fn wait_for_pi(interface: &FuzzerNodeInterface) {
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

    wait_for_pi(interface).await;

    trace!("Powering on the device");
    if let Err(err) = interface.power_button_short().await {
        error!("Failed to power on the device: {:?}", err);
        wait_for_pi(interface).await;
        if let Err(err) = interface.power_button_short().await {
            error!("Failed to power on the device: {:?}", err);
            return false;
        }
        wait_for_pi(interface).await;
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
