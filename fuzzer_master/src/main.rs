use clap::{Parser, Subcommand};
use fuzzer_data::instruction_corpus::InstructionCorpus;
use fuzzer_data::{Ota, OtaC2DTransport, OtaD2C, OtaD2CTransport};
use fuzzer_master::database::Database;
use fuzzer_master::device_connection::DeviceConnection;
use fuzzer_master::fuzzer_node_bridge::FuzzerNodeInterface;
use fuzzer_master::genetic_breeding::BreedingState;
use fuzzer_master::instruction_mutations::InstructionMutState;
use fuzzer_master::manual_execution::ManualExecutionState;
use fuzzer_master::net::{net_reboot_device, net_receive_performance_timing, ExecuteSampleResult};
use fuzzer_master::spec_fuzz::SpecFuzzMutState;
use fuzzer_master::{
    afl_fuzzing, genetic_breeding, guarantee_initial_state, instruction_mutations,
    manual_execution, net, power_on, spec_fuzz, CommandExitResult, P0_FREQ,
};
use hypervisor::state::StateDifference;
use itertools::Itertools;
use log::{error, info, trace};
use performance_timing::TimeMeasurement;
use std::io;
use std::io::Write;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;
use ucode_compiler_dynamic::instruction::Instruction;
use ucode_compiler_dynamic::sequence_word::SequenceWord;

pub mod main_compare;
pub mod main_viewer;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    database: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug, Clone)]
enum Cmd {
    Genetic {
        #[arg(short, long)]
        corpus: Option<PathBuf>,
        #[arg(short, long)]
        timeout_hours: Option<u32>,
        #[arg(short, long)]
        disable_feedback: bool,
    },
    InstructionMutation,
    Init,
    Reboot,
    Cap,
    Performance,
    Spec {
        report: PathBuf,
        #[arg(short, long)]
        all: bool,
        #[arg(short, long)]
        skip: bool,
        #[arg(short, long)]
        no_crbus: bool,
        #[arg(short, long)]
        exclude: Option<PathBuf>,
        #[arg(short, long)]
        fuzzy_pmc: bool,
    },
    SpecManual {
        #[arg(short, long)]
        instruction: Option<Vec<String>>,
        #[arg(short, long)]
        sequence_word: Option<String>,
    },
    Manual {
        #[arg(short, long)]
        input: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(short = 'f', long, default_value = "false")]
        overwrite: bool,
        #[arg(short, long)]
        bulk: bool,
    },
    AFL {
        #[arg(short, long)]
        solutions: Option<PathBuf>,
        #[arg(short, long)]
        corpus: Option<PathBuf>,
        #[arg(short, long)]
        timeout_hours: Option<u32>,
        #[arg(short, long)]
        disable_feedback: bool,
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

    let database_file = args
        .database
        .unwrap_or(PathBuf::from_str("database.json").unwrap());

    let mut database = fuzzer_master::database::Database::from_file(&database_file).map_or_else(
        |e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                let db = Database::empty(&database_file);
                return db;
            }
            println!("Failed to load the database: {:?}", e);
            print!("Do you want to create a new one? (y/n): ");
            io::stdout().flush().unwrap();

            let mut input = String::new();
            io::stdin().read_line(&mut input).unwrap();

            if input.trim().eq_ignore_ascii_case("y") {
                let db = Database::empty(&database_file);
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

    let corpus_vec = if let Cmd::Genetic {
        corpus,
        timeout_hours: _,
        disable_feedback: _,
    } = &args.cmd
    {
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

    if let Cmd::AFL {
        disable_feedback,
        timeout_hours,
        corpus,
        solutions,
    } = &args.cmd
    {
        return afl_fuzzing::afl_main(
            &mut udp,
            &interface,
            &mut database,
            corpus.as_ref().map(|v| v.clone()),
            solutions.as_ref().map(|v| v.clone()),
            *timeout_hours,
            *disable_feedback,
        )
        .await;
    }

    let mut state_breeding = BreedingState::default();
    let mut state_instructions = InstructionMutState::default();
    let mut state_spec_fuzz = SpecFuzzMutState::default();
    let mut state_manual_execution = ManualExecutionState::default();
    let mut continue_count = 0;
    let mut last_time_perf_from_device = Instant::now() - Duration::from_secs(1000000);

    let start_time = Instant::now();
    let mut last_remaining_time = Instant::now();

    loop {
        let timing = TimeMeasurement::begin("host::main_loop");

        let _ = database.save().await.map_err(|e| {
            error!("Failed to save the database: {:?}", e);
        });

        let result = match &args.cmd {
            Cmd::Genetic {
                corpus,
                timeout_hours,
                disable_feedback,
            } => {
                if let Some(timeout) = timeout_hours {
                    if start_time.elapsed().as_secs_f64() / (60.0 * 60.0) > *timeout as f64 {
                        info!("Timeout reached!");
                        break;
                    } else if last_remaining_time.elapsed().as_secs_f64() >= 60.0 {
                        last_remaining_time = Instant::now();
                        info!(
                            "Remaining time: {:0.2}h",
                            start_time.elapsed().as_secs_f64() / 60.0 - (*timeout as f64)
                        );
                    }
                }

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
                    !disable_feedback,
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
            Cmd::Spec {
                report,
                all,
                skip,
                no_crbus,
                exclude,
                fuzzy_pmc,
            } => {
                let _timing = TimeMeasurement::begin("host::spec_fuzz_loop");
                spec_fuzz::main(
                    &mut udp,
                    &mut database,
                    &mut state_spec_fuzz,
                    &report,
                    *all,
                    *skip,
                    *no_crbus,
                    exclude.as_ref(),
                    *fuzzy_pmc,
                )
                .await
            }
            Cmd::SpecManual {
                instruction,
                sequence_word,
            } => {
                let mut instruction = instruction
                    .as_ref()
                    .map(|hex| {
                        hex.iter()
                            .map(|hex| {
                                u64::from_str_radix(hex, 16)
                                    .ok()
                                    .map(Instruction::disassemble)
                            })
                            .collect_vec()
                    })
                    .unwrap_or(vec![Some(Instruction::NOP)]);

                if instruction.len() > 3 {
                    error!("Too many instructions provided");
                    instruction.truncate(3);
                }

                if instruction.iter().any(|x| x.is_none()) {
                    error!("Invalid instruction provided");
                }

                let instruction = instruction
                    .into_iter()
                    .map(|x| x.unwrap_or(Instruction::NOP))
                    .collect_vec();

                let sequence_word = sequence_word
                    .as_ref()
                    .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                    .map(SequenceWord::disassemble)
                    .map(|s| {
                        s.unwrap_or_else(|e| {
                            error!("Invalid sequence word: {:?}", e);
                            SequenceWord::NOP
                        })
                    });

                let result = net::net_speculative_sample(
                    &mut udp,
                    [
                        instruction.get(0).map(|x| *x).unwrap_or(Instruction::NOP),
                        instruction.get(1).map(|x| *x).unwrap_or(Instruction::NOP),
                        instruction.get(2).map(|x| *x).unwrap_or(Instruction::NOP),
                    ],
                    sequence_word.unwrap_or(SequenceWord::NOP),
                    vec![
                        x86_perf_counter::INSTRUCTIONS_RETIRED,
                        x86_perf_counter::MS_DECODED_MS_ENTRY,
                        x86_perf_counter::UOPS_ISSUED_ANY,
                        x86_perf_counter::UOPS_RETIRED_ANY,
                    ],
                )
                .await;

                match result {
                    ExecuteSampleResult::Timeout => {
                        error!("Timeout while executing the sample");
                        CommandExitResult::ExitProgram
                    }
                    ExecuteSampleResult::Rerun => CommandExitResult::RetryOrReconnect,
                    ExecuteSampleResult::Success(mut data) => {
                        for (val, name) in data.perf_counters.iter().zip([
                            "iRetired",
                            "msDecoded",
                            "uOpsIssued",
                            "uOpsRetired",
                        ]) {
                            println!("{}: {}", name, val);
                        }

                        data.arch_before.rflags = 0x202; // a bit hacky, todo: do it properly

                        for (name, before, after) in data.arch_before.difference(&data.arch_after) {
                            println!("DIFF {}: {:#x?} -> {:#x?}", name, before, after);
                        }

                        CommandExitResult::ExitProgram
                    }
                }
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
                bulk,
            } => {
                manual_execution::main(
                    &mut udp,
                    &interface,
                    &mut database,
                    &input,
                    if !bulk {
                        Some(
                            output
                                .as_ref()
                                .map(|v| v.clone())
                                .unwrap_or(input.with_extension("out")),
                        )
                    } else {
                        output.as_ref().map(|v| v.clone())
                    },
                    *overwrite,
                    *bulk,
                    &mut state_manual_execution,
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
            Cmd::AFL { .. } => {
                unreachable!()
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
