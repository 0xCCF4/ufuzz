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
use libafl_bolts::rands::random_seed;
use log::{error, info, trace, warn};
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

/// Main fuzzing application. This app governs and controls the entire fuzzing process,
/// issuing commands to a fuzzer agent (which e.g. executes fuzzing inputs on its CPU)
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// The database file to save fuzzing progress and results to
    #[arg(short, long)]
    database: Option<PathBuf>,
    /// Dont reset address blacklist
    #[arg(long)]
    dont_reset: bool,
    /// Address of the fuzzer instrumentor
    #[arg(long, default_value = "http://10.83.3.198:8000")]
    instrumentor: String,
    /// Address of the fuzzer agent
    #[arg(long, default_value = "10.83.3.6:4444")]
    agent: String,
    /// The command to execute
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug, Clone)]
enum Cmd {
    /// Perform coverage fuzzing using (bad) genetic mutation algorithm, probably you would like to execute the `afl` command.
    /// == Requires the `fuzzer_device` app running on the agent ==
    Genetic {
        /// A corpus file to generate the initial fuzzing inputs from
        #[arg(short, long)]
        corpus: Option<PathBuf>,
        /// After how many hours the fuzzing should be terminated. If not given fuzzing must be interrupted by CTRL+C
        #[arg(short, long)]
        timeout_hours: Option<u32>,
        /// Disable the feedback loop, fuzzing input ratings will become randomized
        #[arg(short, long)]
        disable_feedback: bool,
    },
    /// Under-construction
    InstructionMutation,
    /// Bring up the fuzzer agent to a usable state
    Init,
    /// Reboot the fuzzer agent
    Reboot {
        /// Resets the drive of the fuzzer agent before rebooting
        #[arg(short, long)]
        reset: bool,
    },
    /// Report the capabilities of the fuzzer agent
    Cap {
        /// CPUid leaf (EAX)
        #[arg(long, default_value = "1")]
        leaf: u32,
        /// CPUid specifier (ECX)
        #[arg(long, default_value = "0")]
        node: u32,
    },
    /// Extract performance values from the fuzzer agent
    Performance,
    /// Do speculative microcode fuzzing
    /// == Requires the `spec_fuzz` app running on the agent ==
    Spec {
        /// Path to database; save the results to this file
        report: PathBuf,
        /// Execute fuzzing for all instructions extracted from MSROM
        #[arg(short, long)]
        all: bool,
        /// Skip instruction that were already run; continue a stopped fuzzing execution
        #[arg(short, long)]
        skip: bool,
        /// Skip all CRBUS related instructions
        #[arg(short, long)]
        no_crbus: bool,
        /// Exclude a list of instructions from running through the fuzzer
        #[arg(short, long)]
        exclude: Option<PathBuf>,
        /// Run all PMC variants through the fuzzer; takes a long time
        #[arg(short, long)]
        fuzzy_pmc: bool,
    },
    /// Executes a given speculative fuzzing payload manually
    /// == Requires the `spec_fuzz` app running on the agent ==
    SpecManual {
        /// Instruction given as list of hex values
        #[arg(short, long)]
        instruction: Option<Vec<String>>,
        /// Sequence word given as hex number
        #[arg(short, long)]
        sequence_word: Option<String>,
    },
    /// Executes a single fuzzing input manually
    /// == Requires the `fuzzer_device` app running on the agent ==
    Manual {
        /// Path to the fuzzing input (file with hexadecimally formatted string/fuzzing input)
        #[arg(short, long)]
        input: PathBuf,
        /// Path to save the fuzzing result to
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Overwrite the target file without asking
        #[arg(short = 'f', long, default_value = "false")]
        overwrite: bool,
        /// Split input file at newline characters and regard each line as single fuzzing input
        #[arg(short, long)]
        bulk: bool,
        /// How many iterations should the sample trace execute for
        #[arg(short, long, default_value = "1000")]
        max_iterations: u16,
        /// Analyze which memory locations are executed when tracing the fuzzing sample
        #[arg(short, long)]
        print_mem_access: bool,
        /// Disable coverage collection
        #[arg(short, long)]
        no_coverage_collection: bool,
    },
    /// Executes a corpus of fuzzing inputs; essentially runs the manual command using all files within the given directory
    /// == Requires the `fuzzer_device` app running on the agent ==
    BulkManual {
        /// Path to directory containing fuzzing inputs
        input: Vec<PathBuf>,
        /// Path to directory to which outputs should be written to
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Overwrite without asking
        #[arg(short = 'f', long)]
        overwrite: bool,
        /// Disable coverage collection
        #[arg(short, long)]
        no_coverage_collection: bool,
        /// How many iterations the sample trace should run at most
        #[arg(short, long, default_value = "1000")]
        max_iterations: u16,
    },
    /// Executes the main fuzzing loop with AFL mutations
    /// == Requires the `fuzzer_device` app running on the agent ==
    AFL {
        /// Store findings to this path
        #[arg(short, long)]
        solutions: Option<PathBuf>,
        /// Use the provided corpus file to generate initial fuzzing inputs from
        #[arg(short, long)]
        corpus: Option<PathBuf>,
        /// Store the fuzzing corpus to that path
        #[arg(short, long)]
        afl_corpus: Option<PathBuf>,
        /// End fuzzing after that many hours automatically, if not set fuzzing does not terminate
        #[arg(short, long)]
        timeout_hours: Option<u32>,
        /// Disabled coverage fuzzing feedback; instead fuzzing feedback is randomized
        #[arg(short, long)]
        disable_feedback: bool,
        /// When not using the `corpus` argument; initial fuzzing inputs are random byte sequences; enabling this flag
        /// these byte sequences are selected among printable ASCII characters
        #[arg(short, long)]
        printable_input_generation: bool,
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

    let interface = Arc::new(FuzzerNodeInterface::new(&args.instrumentor));
    let mut udp = DeviceConnection::new(&args.agent)
        .await
        .expect("failed to create agent socket");

    let corpus_vec = if let Cmd::Genetic { corpus, .. } | Cmd::AFL { corpus, .. } = &args.cmd {
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

    if !args.dont_reset {
        for _ in 0..100 {
            let result = udp.send(OtaC2DTransport::ResetBlacklist).await;
            match result {
                Err(err) => {
                    warn!("Failed to reset blacklist: {:?}", err);
                }
                Ok(_) => {
                    break;
                }
            }
        }
    }

    if let Cmd::AFL {
        disable_feedback,
        timeout_hours,
        corpus: _,
        solutions,
        printable_input_generation,
        afl_corpus,
    } = &args.cmd
    {
        return afl_fuzzing::afl_main(
            &mut udp,
            &interface,
            &mut database,
            afl_corpus.as_ref().map(|v| v.clone()),
            corpus_vec,
            solutions.as_ref().map(|v| v.clone()),
            *timeout_hours,
            *disable_feedback,
            random_seed(),
            *printable_input_generation,
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

    let mut bulk_manual_queue = if let Cmd::BulkManual { input, .. } = &args.cmd {
        input.clone()
    } else {
        vec![]
    };

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
            Cmd::Cap { leaf, node } => {
                let _ = udp
                    .send(OtaC2DTransport::GetCapabilities {
                        leaf: *leaf,
                        node: *node,
                    })
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
                max_iterations,
                print_mem_access,
                no_coverage_collection,
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
                    *max_iterations,
                    *print_mem_access,
                    !no_coverage_collection,
                )
                .await
            }
            Cmd::BulkManual {
                overwrite,
                output,
                no_coverage_collection,
                max_iterations,
                ..
            } => {
                if bulk_manual_queue.is_empty() {
                    CommandExitResult::ExitProgram
                } else {
                    let input = bulk_manual_queue.get(0).unwrap();
                    let result = manual_execution::main(
                        &mut udp,
                        &interface,
                        &mut database,
                        input,
                        Some(
                            output
                                .as_ref()
                                .cloned()
                                .unwrap_or_else(|| input.with_extension("out")),
                        ),
                        *overwrite,
                        false,
                        &mut state_manual_execution,
                        *max_iterations,
                        false,
                        !no_coverage_collection,
                    )
                    .await;
                    match result {
                        CommandExitResult::ExitProgram => {
                            bulk_manual_queue.remove(0);
                            state_manual_execution = ManualExecutionState::default();
                            if bulk_manual_queue.is_empty() {
                                CommandExitResult::ExitProgram
                            } else {
                                CommandExitResult::Operational
                            }
                        }
                        x => x,
                    }
                }
            }
            Cmd::Init => {
                let x = udp.send(OtaC2DTransport::AreYouThere).await;
                if let Err(_) = x {
                    CommandExitResult::RetryOrReconnect
                } else {
                    CommandExitResult::ExitProgram
                }
            }
            Cmd::Reboot { reset } => {
                if !reboot_state {
                    if *reset {
                        for _ in 0..100 {
                            let result = udp.send(OtaC2DTransport::ResetBlacklist).await;
                            match result {
                                Err(err) => {
                                    warn!("Failed to reset blacklist: {:?}", err);
                                }
                                Ok(_) => {
                                    break;
                                }
                            }
                        }
                    }
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
                unreachable!("if statement above already governs this path")
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
