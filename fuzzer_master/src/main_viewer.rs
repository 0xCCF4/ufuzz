use clap::Parser;
use coverage::harness::coverage_harness::hookable_address_iterator::HookableAddressIterator;
use coverage::harness::coverage_harness::modification_engine::ModificationEngineSettings;
use data_types::addresses::Address;
use fuzzer_master::database::CodeEvent;
use hypervisor::state::StateDifference;
use itertools::Itertools;
use std::collections::{BTreeMap, BTreeSet};
use std::io::BufWriter;
use std::io::Write;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    database: Option<PathBuf>,
    plot_path: Option<PathBuf>,
}

pub fn main() {
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
                println!("Sample: {}", code_to_hex_string(result.code.as_slice()));
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
                println!("Sample: {}", code_to_hex_string(result.code.as_slice()));
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
                println!("Sample: {}", code_to_hex_string(result.code.as_slice()));
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
                println!("Sample: {}", code_to_hex_string(result.code.as_slice()));
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
                println!();
            }
        }
    }

    println!("\n\nCoverage summary:");
    let possible_cov_points = HookableAddressIterator::construct(
        &ucode_dump::dump::ROM_cpu_000506CA,
        &ModificationEngineSettings::default(),
        1,
        |a| {
            db.data
                .blacklisted_addresses
                .iter()
                .all(|b| a.address() != b.address as usize)
        },
    );
    let mut coverage = BTreeSet::new();
    for result in &db.data.results {
        for cov in result.coverage.iter().filter(|x| *x.1 > 0) {
            coverage.insert(*cov.0);
        }
    }
    let mut coverage_normalized = BTreeSet::new();
    for cov in coverage.iter() {
        if !db.blacklisted().contains(cov) {
            coverage_normalized.insert(*cov);
        }
    }
    println!(
        "Total coverage: {} ({})",
        coverage.len(),
        coverage_normalized.len()
    );
    println!(" - Possible coverage points: {}", possible_cov_points.len());
    println!(
        " - Percentage: {:.2}%",
        coverage.len() as f64 / possible_cov_points.len() as f64 * 100.0
    );

    let seeds = db
        .data
        .results
        .iter()
        .map(|x| x.found_at.iter().map(|x| x.seed))
        .flatten()
        .unique()
        .collect::<Vec<_>>();

    println!("Coverage by seed:");
    for seed in seeds {
        println!(" - {}", seed);

        let largest_evolution = db
            .data
            .results
            .iter()
            .filter(|x| x.found_at.iter().any(|x| x.seed == seed))
            .map(|x| x.found_at.iter().map(|x| x.evolution))
            .flatten()
            .max()
            .unwrap_or(0);

        let mut overall_coverage = Vec::new();
        for evolution in 0..largest_evolution {
            let mut coverage = BTreeMap::new();
            let results = db
                .data
                .results
                .iter()
                .filter(|x| {
                    x.found_at
                        .iter()
                        .any(|x| x.seed == seed && x.evolution == evolution)
                })
                .map(|x| x.coverage.iter().filter(|x| *x.1 > 0))
                .flatten();
            for result in results {
                if *result.1 > 0 {
                    *coverage.entry(*result.0).or_insert(0) += result.1;
                }
            }
            overall_coverage.push(coverage);
        }
        print!("   - Unique coverage: ");
        for (index, coverage) in overall_coverage.iter().enumerate() {
            print!("{}", coverage.len());
            if index != overall_coverage.len() - 1 {
                print!(", ");
            }
        }
        println!();
        print!("   - Total coverage: ");
        for (index, coverage) in overall_coverage.iter().enumerate() {
            print!("{}", coverage.iter().map(|x| *x.1 as u64).sum::<u64>());
            if index != overall_coverage.len() - 1 {
                print!(", ");
            }
        }
        println!();
    }

    if let Some(file) = args.plot_path {
        let file_cov =
            std::fs::File::create(file.with_extension("cov.csv")).expect("Could not open file");
        let mut writer_cov = BufWriter::new(file_cov);

        let file_time =
            std::fs::File::create(file.with_extension("time.csv")).expect("Could not open file");
        let mut writer_time = BufWriter::new(file_time);

        let mut unique_coverage: BTreeSet<u16> = BTreeSet::new();
        for (i, program) in db.data.results.iter().enumerate() {
            for entry in program
                .coverage
                .keys()
                .filter(|p| !unique_coverage.contains(p))
            {
                writeln!(&mut writer_time, "{entry}, {i}").expect("Could not write to file");
            }

            let prev = unique_coverage.len();
            unique_coverage.extend(program.coverage.keys());
            if prev != unique_coverage.len() {
                writeln!(&mut writer_cov, "{i}, {}", unique_coverage.len())
                    .expect("Could not write to coverage writer");
            }
        }
    }
}

fn code_to_hex_string(code: &[u8]) -> String {
    code.iter().map(|x| format!("{:02x}", x)).join(" ")
}
