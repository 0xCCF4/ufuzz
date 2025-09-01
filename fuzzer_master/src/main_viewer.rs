//! Fuzzing database viewer module
//!
//! This module provides functionality for viewing and analyzing fuzzing results.

use clap::Parser;
use coverage::harness::coverage_harness::modification_engine::{
    modify_triad_for_hooking, ModificationEngineSettings,
};
use data_types::addresses::UCInstructionAddress;
use fuzzer_master::database::{BlacklistEntry, CodeEvent, ExcludeType, Timestamp};
use hypervisor::state::{StateDifference, VmExitReason};
use itertools::Itertools;
use log::error;
use std::collections::{BTreeMap, BTreeSet};
use std::io::BufWriter;
use std::io::Write;
use std::path::PathBuf;

/// Command-line arguments for the viewer tool
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// If set, only coverage information will be plotted
    #[arg(short, long)]
    just_coverage: bool,
    /// Additional blacklist files from fuzzer_device
    #[arg(short, long)]
    additional_blacklists: Option<PathBuf>,
    /// Path to the database file
    database: Option<PathBuf>,
    /// Path for plotting output, if not provided, no plotting will be done
    plot_path: Option<PathBuf>,
}

/// Main entry point for viewing fuzzing results
///
/// This function loads and analyzes fuzzing results from a database.
pub fn main() {
    env_logger::init();

    if let Err(err) = performance_timing::initialize(54_000_000.0) {
        error!("Failed to initialize performance timing: {:?}", err);
        return;
    }

    let args = Args::parse();

    let db_path = args
        .database
        .unwrap_or_else(|| PathBuf::from("database.json"));

    if !db_path.exists() {
        eprintln!("Database file does not exist");
        return;
    }

    let mut db = fuzzer_master::database::Database::from_file(&db_path).unwrap_or_else(|e| {
        eprintln!("Failed to load the database: {:?}", e);
        std::process::exit(1)
    });

    if let Some(blacklist_directory) = args.additional_blacklists {
        if !blacklist_directory.exists() {
            eprintln!("Additional blacklist directory does not exist");
            return;
        }

        for file in std::fs::read_dir(blacklist_directory).unwrap_or_else(|e| {
            eprintln!("Failed to read additional blacklist directory: {:?}", e);
            std::process::exit(1)
        }) {
            let file = file.expect("Failed to get file");
            if file.path().extension().and_then(|s| s.to_str()) == Some("txt") {
                println!("Loading additional blacklist from {:?}", file.path());
                let content = std::fs::read(file.path()).unwrap_or_else(|e| {
                    eprintln!("Failed to read file {:?}: {:?}", file.path(), e);
                    std::process::exit(1)
                });
                let content = content
                    .iter()
                    .filter(|x| **x != 0)
                    .filter(|x| x.is_ascii_hexdigit() || x.is_ascii_whitespace())
                    .map(|x| *x as char)
                    .collect::<String>();
                let additional_blacklist: BTreeSet<u16> = content
                    .lines()
                    .filter_map(|line| u16::from_str_radix(line.trim(), 16).ok())
                    .collect();
                println!(
                    " -> {} additional blacklisted addresses: {:x?}",
                    additional_blacklist.len(),
                    additional_blacklist
                );
                for address in additional_blacklist {
                    if !db.blacklisted().contains(&address) {
                        db.data.blacklisted_addresses.push(BlacklistEntry {
                            address,
                            code: Vec::new(),
                            iteration: u32::MAX,
                            exclude_type: ExcludeType::Normal,
                        })
                    }
                }
            }
        }
    }

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
                if *serialized_exit == Some(VmExitReason::TimerExpiration)
                    || result.exit == VmExitReason::TimerExpiration
                {
                    continue;
                }

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
            if let CodeEvent::SerializedMismatch {
                serialized_exit,
                serialized_state: _,
                code: _,
            } = event
            {
                if *serialized_exit == Some(VmExitReason::TimerExpiration)
                    || result.exit == VmExitReason::TimerExpiration
                {
                    continue;
                }
            }
        }

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
    let mut problematic_addresses = BTreeMap::new();
    let mut problematic_samples = BTreeSet::new();
    for result in &db.data.results {
        for event in &result.events {
            if let CodeEvent::CoverageProblem {
                address,
                coverage_exit,
                coverage_state,
            } = event
            {
                if matches!(coverage_exit, Some(VmExitReason::TimerExpiration))
                    | matches!(result.exit, VmExitReason::TimerExpiration)
                {
                    continue;
                }

                *problematic_addresses.entry(*address).or_insert(0) += 1u32;
                problematic_samples.insert(result.code.clone());

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

    println!("\n\nProblematic Addresses:");
    let mut sorted_addresses: Vec<_> = problematic_addresses.iter().collect();
    sorted_addresses.sort_by(|a, b| b.1.cmp(a.1));

    for (address, count) in sorted_addresses {
        println!(" - {:#x}: {}", address, count);
    }

    println!("\n\nProblematic samples:");
    for result in problematic_samples {
        println!(" {}", code_to_hex_string(result.as_slice()));
    }

    println!("\n\nCoverage summary:");
    let mut total_addresses_hookable = 0;
    // let mut total_addresses_hookable_excluded = 0;
    for i in 0..0x7c00 {
        let address = UCInstructionAddress::from_const(i);
        if address.triad_offset() != 0 {
            continue;
        }

        let low_hookable = modify_triad_for_hooking(
            address.triad_base(),
            &ucode_dump::dump::ROM_cpu_000506CA,
            &ModificationEngineSettings::default(),
        )
        .is_ok();
        let high_hookable = modify_triad_for_hooking(
            address.triad_base() + 2,
            &ucode_dump::dump::ROM_cpu_000506CA,
            &ModificationEngineSettings::default(),
        )
        .is_ok();

        if low_hookable {
            total_addresses_hookable += 2;
            /* total_addresses_hookable_excluded += db
                .blacklisted()
                .contains(&(address.address() as u16))
                .then_some(1)
                .unwrap_or(0);
            total_addresses_hookable_excluded += db
                .blacklisted()
                .contains(&(address.address() as u16 + 1))
                .then_some(1)
                .unwrap_or(0); */
        }
        if high_hookable {
            total_addresses_hookable += 1;
            /* total_addresses_hookable_excluded += db
            .blacklisted()
            .contains(&(address.address() as u16 + 2))
            .then_some(1)
            .unwrap_or(0); */
        }
    }
    let mut coverage = BTreeSet::new();
    for result in &db.data.results {
        for cov in result.coverage.iter().filter(|x| *x.1 > 0) {
            if *cov.0 >= 0x1000 && cfg!(feature = "__debug_only_below_0x1000") {
                continue;
            }
            coverage.insert(*cov.0);
        }
    }
    let mut blacklist_enhance = 0;
    for address in db.blacklisted() {
        blacklist_enhance += coverage.insert(address).then_some(1).unwrap_or(0);
    }
    let mut coverage_normalized = BTreeSet::new();
    for cov in coverage.iter() {
        if !db.blacklisted().contains(cov)
            && (*cov < 0x1000 || !cfg!(feature = "__debug_only_below_0x1000"))
        {
            coverage_normalized.insert(*cov);
        }
    }
    println!(
        "Total coverage: {} ({}) (+{})",
        coverage.len(),
        coverage_normalized.len(),
        blacklist_enhance
    );
    println!(" - Excluded: {}", db.blacklisted().collect_vec().len());
    println!(
        " - Possible coverage addresses: {}",
        total_addresses_hookable
    );
    println!(
        " - Percentage: {:.2}%",
        coverage.len() as f64 / total_addresses_hookable as f64 * 100.0
    );

    let seeds = db
        .data
        .results
        .iter()
        .map(|x| x.found_at.iter().map(|x| x.seed))
        .flatten()
        .unique()
        .collect::<Vec<_>>();

    if !args.just_coverage {
        println!("Coverage by seed:");
        let mut unique_cov = BTreeSet::new();
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
            let mut unique_cov_counts = Vec::new();
            for evolution in 1..largest_evolution + 1 {
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

                let mut unique_cov_count = 0;
                for result in results {
                    if *result.0 >= 0x1000 && cfg!(feature = "__debug_only_below_0x1000") {
                        continue;
                    }
                    if *result.1 > 0 {
                        *coverage.entry(*result.0).or_insert(0) += result.1;
                        if unique_cov.insert(result.0) {
                            unique_cov_count += 1u32;
                        }
                    }
                }
                unique_cov_counts.push(unique_cov_count);
                overall_coverage.push(coverage);
            }
            print!("   - Delta coverage: ");
            for (index, coverage) in unique_cov_counts.iter().enumerate() {
                print!("+{}", coverage);
                if index != unique_cov_counts.len() - 1 {
                    print!(", ");
                }
            }
            println!();
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
    }

    let mut acc = db.data.performance.normalize();
    acc.data.push(db.data.device_performance.accumulate());
    println!("{}", acc);

    if let Some(file) = args.plot_path {
        let file_cov =
            std::fs::File::create(file.with_extension("cov.csv")).expect("Could not open file");
        let mut writer_cov = BufWriter::new(file_cov);

        let file_time =
            std::fs::File::create(file.with_extension("time.csv")).expect("Could not open file");
        let mut writer_time = BufWriter::new(file_time);

        let mut unique_coverage: BTreeMap<u16, u32> = BTreeMap::new(); // coverage found at
        let found_first = db.data.results.iter().map(|x| &x.found_on).flatten().min();
        for (i, program) in db.data.results.iter().enumerate() {
            let code = &program.code;

            for entry in program
                .coverage
                .keys()
                .filter(|p| unique_coverage.get(p).is_none())
            {
                writeln!(&mut writer_time, "{entry}, {i}, {code:?}")
                    .expect("Could not write to file");
            }

            let prev = unique_coverage.len();
            for key in program.coverage.keys() {
                unique_coverage.entry(*key).or_insert(i as u32);
            }
            if prev != unique_coverage.len() {
                writeln!(
                    &mut writer_cov,
                    "{i}, {}, {}, {}",
                    unique_coverage.len(),
                    program
                        .found_at
                        .iter()
                        .map(|x| x.evolution)
                        .min()
                        .unwrap_or(0),
                    program
                        .found_on
                        .iter()
                        .min()
                        .unwrap_or(&Timestamp::ZERO)
                        .seconds()
                        .saturating_sub(found_first.map(|x| x.seconds()).unwrap_or(0))
                )
                .expect("Could not write to coverage writer");
            }
        }

        if args.just_coverage {
            return;
        }

        drop(writer_cov);
        let file_time = std::fs::File::create(file.with_extension("unique_cov.csv"))
            .expect("Could not open file");
        let mut writer_time = BufWriter::new(file_time);

        let total_cov_file = std::fs::File::create(file.with_extension("total_cov.csv"))
            .expect("Could not open file");
        let mut total_cov_file = BufWriter::new(total_cov_file);

        for c in 0..0x7c00u16 {
            let entry = unique_coverage.get(&c);
            writeln!(
                &mut writer_time,
                "{c}, {}",
                entry.map(|v| v.to_string()).unwrap_or("".to_string())
            )
            .expect("Could not write to file");

            let mut total_coverage: u64 = 0;
            for sample in db.data.results.iter() {
                total_coverage += *sample.coverage.get(&c).unwrap_or(&0) as u64;
            }
            writeln!(&mut total_cov_file, "{c}, {total_coverage}")
                .expect("Could not write to file");
        }
    }
}

/// Converts a byte slice to a hexadecimal string representation
///
/// # Arguments
///
/// * `code` - Byte slice to convert
///
/// # Returns
///
/// A string containing the hexadecimal representation of the code
fn code_to_hex_string(code: &[u8]) -> String {
    code.iter().map(|x| format!("{:02x}", x)).join(" ")
}
