use clap::Parser;
use fuzzer_master::database::CodeEvent;
use hypervisor::state::StateDifference;
use itertools::Itertools;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    database: Option<PathBuf>,
}

fn main() {
    env_logger::init();

    let args = Args::parse();

    let db_path = args
        .database
        .unwrap_or_else(|| PathBuf::from("database.json"));

    if !db_path.exists() {
        eprintln!("Database file does not exist");
        return;
    }

    let db = fuzzer_master::database::Database::from_file(&db_path).unwrap_or_else(|e| {
        eprintln!("Failed to load the database: {:?}", e);
        std::process::exit(1)
    });

    println!("Found {} samples in the database", db.data.results.len());

    println!("Likely bugs:");
    for result in &db.data.results {
        for event in &result.events {
            if let CodeEvent::VeryLikelyBug { code } = event {
                print!("Sample: {}", code_to_hex_string(result.code.as_slice()));
                println!("Serialized: {}", code_to_hex_string(code.as_slice()))
            }
        }
    }

    println!("\n\nSerialize mismatches:");
    for result in &db.data.results {
        for event in &result.events {
            if let CodeEvent::SerializedMismatch {
                serialized_exit,
                serialized_state,
                code,
            } = event
            {
                print!("Sample: {}", code_to_hex_string(result.code.as_slice()));
                println!("Serialized: {}", code_to_hex_string(code.as_slice()));

                if let Some(serialized_exit) = serialized_exit {
                    println!("Serialized exit: {:x?}", serialized_exit);
                    println!("Normal exit: {:x?}", result.exit);
                }

                if let Some(serialized_state) = serialized_state {
                    println!("State difference:");
                    for (name, a, b) in result.state.difference(&serialized_state) {
                        println!(" - {}: {:#x?} -> {:#x?}", name, a, b);
                    }
                }
            }
        }
    }

    println!("\n\nState trace differences:");
    for result in &db.data.results {
        for event in &result.events {
            if let CodeEvent::StateTraceMismatch {
                index,
                code,
                serialized,
                normal,
            } = event
            {
                print!("Sample: {}", code_to_hex_string(result.code.as_slice()));
                println!("Serialized: {}", code_to_hex_string(code.as_slice()));
                println!("State trace mismatch at index {}", index);

                if let (Some(normal), Some(serialized)) = (&normal, &serialized) {
                    println!("State difference:");
                    for (name, a, b) in normal.difference(serialized) {
                        println!(" - {}: {:#x?} -> {:#x?}", name, a, b);
                    }
                } else {
                    if serialized.is_none() {
                        println!("Serialized state is missing");
                    }
                    if normal.is_none() {
                        println!("Normal state is missing");
                    }
                }
            }
        }
    }

    println!("\n\nCoverage problems:");
    for result in &db.data.results {
        for event in &result.events {
            if let CodeEvent::CoverageProblem {
                address,
                coverage_exit,
                coverage_state,
            } = event
            {
                print!("Sample: {}", code_to_hex_string(result.code.as_slice()));
                println!("Coverage problem at address {:#x}", address);

                if let Some(coverage_exit) = coverage_exit {
                    println!("Coverage exit: {:x?}", coverage_exit);
                    println!("Normal exit: {:x?}", result.exit);
                }

                if let Some(coverage_state) = coverage_state {
                    println!("State difference:");
                    for (name, a, b) in result.state.difference(&coverage_state) {
                        println!(" - {}: {:#x?} -> {:#x?}", name, a, b);
                    }
                }
            }
        }
    }
}

fn code_to_hex_string(code: &[u8]) -> String {
    code.iter().map(|x| format!("{:02x}", x)).join(" ")
}
