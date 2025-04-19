use clap::{Parser, Subcommand};
use fuzzer_data::instruction_corpus::InstructionCorpus;
use fuzzer_data::{Ota, OtaC2DTransport, OtaD2C, OtaD2CTransport};
use fuzzer_master::database::Database;
use fuzzer_master::device_connection::DeviceConnection;
use fuzzer_master::fuzzer_node_bridge::FuzzerNodeInterface;
use fuzzer_master::genetic_breeding::BreedingState;
use fuzzer_master::instruction_mutations::InstructionMutState;
use fuzzer_master::net::{net_reboot_device, net_receive_performance_timing};
use fuzzer_master::spec_fuzz::SpecFuzzMutState;
use fuzzer_master::{
    genetic_breeding, instruction_mutations, manual_execution, spec_fuzz, wait_for_device,
    CommandExitResult, WaitForDeviceResult, P0_FREQ,
};
use itertools::Itertools;
use log::{error, info, trace, warn};
use performance_timing::{track_time, TimeMeasurement};
use std::io;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;

pub mod main_compare;
pub mod main_viewer;

pub const DATABASE_FILE: &str = "database.json";

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug, Clone)]
enum Cmd {
    Genetic {
        corpus: Option<PathBuf>,
    },
    InstructionMutation,
    Init,
    Reboot,
    Cap,
    Performance,
    Spec,
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

    if let Err(err) = performance_timing::initialize(P0_FREQ) {
        error!("Failed to initialize performance timing: {:?}", err);
        return;
    }

    let mut database = fuzzer_master::database::Database::from_file(DATABASE_FILE).map_or_else(
        |e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                let db = Database::empty(DATABASE_FILE);
                return db;
            }
            println!("Failed to load the database: {:?}", e);
            print!("Do you want to create a new one? (y/n): ");
            io::stdout().flush().unwrap();

            let mut input = String::new();
            io::stdin().read_line(&mut input).unwrap();

            if input.trim().eq_ignore_ascii_case("y") {
                let db = Database::empty(DATABASE_FILE);
                db
            } else {
                std::process::exit(1);
            }
        },
        |x| x,
    );
    info!("Loaded database from {:?}", &database.path);

    let interface = Arc::new(FuzzerNodeInterface::new("http://10.83.3.198:8000"));
    let mut udp = DeviceConnection::new("10.83.3.6:4444").await.unwrap();

    let corpus_vec = if let Cmd::Genetic { corpus } = &args.cmd {
        if let Some(corpus) = corpus {
            let file_reader = match std::fs::File::open(&corpus) {
                Ok(f) => f,
                Err(e) => {
                    error!("Failed to open the corpus file: {:?}", e);
                    return;
                }
            };
            let buf_reader = std::io::BufReader::new(file_reader);
            let result: serde_json::Result<InstructionCorpus> = serde_json::from_reader(buf_reader);
            match result {
                Ok(corpus) => {
                    info!("Loaded corpus");
                    let data = corpus.instructions.into_iter().collect_vec();
                    Some(data)
                }
                Err(e) => {
                    error!("Failed to load the corpus: {:?}", e);
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };

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
    /* TODO
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
    */

    let mut state_breeding = BreedingState::default();
    let mut state_instructions = InstructionMutState::default();
    let mut state_spec_fuzz = SpecFuzzMutState::default();
    let mut continue_count = 0;
    let mut last_time_perf_from_device = Instant::now() - Duration::from_secs(1000000);

    loop {
        let timing = TimeMeasurement::begin("host::main_loop");

        let _ = database.save().await.map_err(|e| {
            error!("Failed to save the database: {:?}", e);
        });

        let result = match &args.cmd {
            Cmd::Genetic { corpus } => {
                let _timing = TimeMeasurement::begin("host::fuzzing_loop");
                if corpus.is_some() && corpus_vec.is_none() {
                    error!("Corpus file is not loaded");
                    return;
                }
                genetic_breeding::main(
                    &mut udp,
                    &interface,
                    &mut database,
                    &mut state_breeding,
                    corpus.as_ref().map(|_v| corpus_vec.as_ref().unwrap()),
                )
                .await
            }
            Cmd::InstructionMutation => {
                let _timing = TimeMeasurement::begin("host::fuzzing_loop");
                instruction_mutations::main(
                    &mut udp,
                    &interface,
                    &mut database,
                    &mut state_instructions,
                )
                .await
            }
            Cmd::Spec => {
                let _timing = TimeMeasurement::begin("host::spec_fuzz_loop");
                spec_fuzz::main(&mut udp, &interface, &mut database, &mut state_spec_fuzz).await
            }
            Cmd::Cap => {
                let _ = udp
                    .send(OtaC2DTransport::GetCapabilities)
                    .await
                    .map_err(|e| {
                        error!("Failed to send GetCapabilities: {:?}", e);
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
                            pmc_number,
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
                    println!(" - PMC count: {}", pmc_number);
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
            Cmd::Performance => {
                let x = udp.send(OtaC2DTransport::AreYouThere).await;
                if let Err(_) = x {
                    CommandExitResult::RetryOrReconnect
                } else {
                    let data =
                        net_receive_performance_timing(&mut udp, Duration::from_secs(5)).await;

                    let mut acc = database.data.performance.normalize();

                    if let Some(data) = data {
                        acc.data
                            .last_mut()
                            .as_mut()
                            .unwrap()
                            .extend(data.into_iter());
                    }

                    println!("{}", acc);

                    CommandExitResult::ExitProgram
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
                    if let Some(state) = net_reboot_device(&mut udp, &interface).await {
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

        if last_time_perf_from_device.elapsed() > Duration::from_secs(60 * 60) {
            // each 60 min
            trace!("Querying device performance values");
            let perf = net_receive_performance_timing(&mut udp, Duration::from_secs(5)).await;
            if let Some(perf) = perf {
                last_time_perf_from_device = Instant::now();
                database.set_device_performance(perf.into());
            }
        }

        let _ = timing.stop();
    }

    let _ = database.save().await.map_err(|e| {
        error!("Failed to save the database: {:?}", e);
    });
}

#[track_time("host::guarantee_state")]
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

#[track_time("host::guarantee_state")]
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
