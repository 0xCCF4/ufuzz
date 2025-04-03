use clap::{Parser, Subcommand};
use fuzzer_data::{Ota, OtaC2DTransport, OtaD2C, OtaD2CTransport};
use fuzzer_master::database::Database;
use fuzzer_master::device_connection::DeviceConnection;
use fuzzer_master::fuzzer_node_bridge::FuzzerNodeInterface;
use fuzzer_master::genetic_breeding::{net_reboot_device, BreedingState};
use fuzzer_master::{
    genetic_breeding, manual_execution, wait_for_device, CommandExitResult, WaitForDeviceResult,
};
use log::{error, info, trace, warn};
use std::io;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

pub mod main_compare;
pub mod main_viewer;

pub const DATABASE_FILE: &str = "database.json";

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand, Debug, Clone, Default)]
enum Cmd {
    Viewer,
    Compare,
    Convert,
    #[default]
    Genetic,
    Init,
    Reboot,
    Cap,
    Manual {
        #[arg(short, long)]
        input: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(short = 'f', long, default_value = "false")]
        overwrite: bool,
    },
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let args = Args::parse();
    let mut reboot_state = false;

    let mut database = fuzzer_master::database::Database::from_file(DATABASE_FILE).map_or_else(
        |e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                let mut db = Database::default();
                db.path = PathBuf::from(DATABASE_FILE);
                return db;
            }
            println!("Failed to load the database: {:?}", e);
            print!("Do you want to create a new one? (y/n): ");
            io::stdout().flush().unwrap();

            let mut input = String::new();
            io::stdin().read_line(&mut input).unwrap();

            if input.trim().eq_ignore_ascii_case("y") {
                let mut db = Database::default();
                db.path = PathBuf::from(DATABASE_FILE);
                db
            } else {
                std::process::exit(1);
            }
        },
        |x| x,
    );
    info!("Loaded database from {:?}", &database.path);

    if let Some(Cmd::Viewer) = &args.cmd {
        main_viewer::main();
        return;
    }
    if let Some(Cmd::Compare) = &args.cmd {
        main_compare::main();
        return;
    }
    if let Some(Cmd::Convert) = &args.cmd {
        // main_convert::main();
        return;
    }

    let interface = Arc::new(FuzzerNodeInterface::new("http://10.83.3.198:8000"));
    let mut udp = DeviceConnection::new("10.83.3.6:4444").await.unwrap();

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
    if database.data.blacklisted_addresses.len() == 0 {
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
            database.data.blacklisted_addresses.extend(addresses);
            database.mark_dirty();
        }
        let _ = database.save().await.map_err(|e| {
            error!("Failed to save the database: {:?}", e);
        });
    }

    let mut state = BreedingState::default();

    let mut continue_count = 0;

    loop {
        let _ = database.save().await.map_err(|e| {
            error!("Failed to save the database: {:?}", e);
        });

        let result = match args.cmd.as_ref().unwrap_or(&Cmd::default()) {
            Cmd::Viewer => CommandExitResult::ExitProgram,
            Cmd::Compare => CommandExitResult::ExitProgram,
            Cmd::Convert => CommandExitResult::ExitProgram,
            Cmd::Genetic => {
                genetic_breeding::main(&mut udp, &interface, &mut database, &mut state).await
            }
            Cmd::Cap => {
                let _ = udp
                    .send(OtaC2DTransport::GetCapabilities)
                    .await
                    .map_err(|e| {
                        error!("Failed to send AreYouThere: {:?}", e);
                    });

                if let Ok(Some(Ota::Transport {
                    content:
                        OtaD2CTransport::Capabilities {
                            coverage_collection,
                            manufacturer,
                            processor_version_eax,
                            processor_version_ebx,
                            processor_version_ecx,
                            processor_version_edx,
                        },
                    ..
                })) = udp
                    .receive_packet(
                        |p| {
                            matches!(
                                p,
                                OtaD2C::Transport {
                                    content: OtaD2CTransport::Capabilities { .. },
                                    ..
                                }
                            )
                        },
                        Some(Duration::from_secs(3)),
                    )
                    .await
                {
                    println!("Capabilities:");
                    println!(" - Coverage collection: {}", coverage_collection);
                    println!(" - Manufacturer: {}", manufacturer);
                    println!(
                        " - Processor version: {:#x} {:#x} {:#x} {:#x}",
                        processor_version_eax,
                        processor_version_ebx,
                        processor_version_ecx,
                        processor_version_edx
                    );
                    CommandExitResult::ExitProgram
                } else {
                    CommandExitResult::RetryOrReconnect
                }
            }
            Cmd::Manual {
                input,
                output,
                overwrite,
            } => {
                manual_execution::main(
                    &mut udp,
                    &interface,
                    &mut database,
                    &input,
                    output
                        .as_ref()
                        .map(|v| v.clone())
                        .unwrap_or(input.with_extension("out")),
                    *overwrite,
                )
                .await
            }
            Cmd::Init => {
                let x = udp.send(OtaC2DTransport::AreYouThere).await;
                if let Err(_) = x {
                    CommandExitResult::RetryOrReconnect
                } else {
                    CommandExitResult::ExitProgram
                }
            }
            Cmd::Reboot => {
                if !reboot_state {
                    reboot_state = true;
                    continue_count = 0;
                    if let Some(state) = net_reboot_device(&mut udp,  &interface).await {
                        state
                    } else {
                        CommandExitResult::Operational
                    }
                } else {
                    let x = udp.send(OtaC2DTransport::AreYouThere).await;
                    if let Err(_) = x {
                        CommandExitResult::RetryOrReconnect
                    } else {
                        CommandExitResult::ExitProgram
                    }
                }
            }
        };

        let connection_lost = match result {
            CommandExitResult::ForceReconnect => {
                continue_count = 0;
                true
            }
            CommandExitResult::RetryOrReconnect => {
                continue_count += 1;
                continue_count > 3
            }
            CommandExitResult::Operational => {
                continue_count = 0;
                false
            }
            CommandExitResult::ExitProgram => {
                info!("Exiting");
                break;
            }
        };

        if connection_lost {
            info!("Connection lost");

            let _ = interface.power_button_long().await;

            // wait for pi reboot

            let _ = power_on(&interface).await;

            tokio::time::sleep(Duration::from_secs(5)).await;

            guarantee_initial_state(&interface, &mut udp).await;
        }
    }

    let _ = database.save().await.map_err(|e| {
        error!("Failed to save the database: {:?}", e);
    });
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

async fn power_on(interface: &FuzzerNodeInterface) -> bool {
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
