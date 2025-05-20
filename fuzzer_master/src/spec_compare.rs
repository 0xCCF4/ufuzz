use clap::Parser;
use fuzzer_master::spec_fuzz::{SpecReport, SpecResult};
use std::cmp::PartialEq;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::BufReader;
use std::io::Write;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    output_unstable: Option<PathBuf>,
    sources: Vec<PathBuf>,
}

#[derive(Debug, PartialEq)]
pub enum ResultFlag {
    StableTimeout,
    StableRuns,
    Unstable,
}

pub fn main() {
    env_logger::init();

    let args = Args::parse();

    if args.sources.len() < 1 {
        println!("Expected at least one source file");
        std::process::exit(1);
    }

    let mut parsed = Vec::new();
    for file in args.sources.iter() {
        let data: SpecReport = match serde_json::from_reader(BufReader::new(
            File::open(file).expect("File does not exists"),
        )) {
            Ok(parsed) => parsed,
            Err(e) => {
                println!("Failed to parse file {:?}: {}", file, e);
                std::process::exit(1);
            }
        };
        parsed.push(data.opcodes);
    }

    let mut compare_against = BTreeMap::new();
    let mut resulting_grade = BTreeMap::new();

    for run in parsed.iter() {
        for (key, value) in run.iter() {
            let compare_value = compare_against.entry(key).or_insert(value.clone());

            let current_grade = resulting_grade.entry(key).or_insert(match compare_value {
                SpecResult::Timeout => ResultFlag::StableTimeout,
                SpecResult::Executed { .. } => ResultFlag::StableRuns,
            });

            if compare_value.is_same_kind(value) {
                // nothing
            } else {
                *current_grade = ResultFlag::Unstable;
            }
        }
    }

    let count_stable_run = resulting_grade
        .values()
        .filter(|x| **x == ResultFlag::StableRuns)
        .count();
    let count_stable_timeout = resulting_grade
        .values()
        .filter(|x| **x == ResultFlag::StableTimeout)
        .count();
    let count_unstable = resulting_grade
        .values()
        .filter(|x| **x == ResultFlag::Unstable)
        .count();

    println!("Results:");
    println!("  Stable runs: {}", count_stable_run);
    println!("  Stable timeouts: {}", count_stable_timeout);
    println!("  Unstable: {}", count_unstable);

    if let Some(output_unstable) = args.output_unstable {
        let file = File::create(output_unstable).expect("Failed to create output file");
        let mut buf_writer = std::io::BufWriter::new(file);
        for (key, value) in resulting_grade.iter() {
            if matches!(value, ResultFlag::Unstable | ResultFlag::StableTimeout) {
                writeln!(
                    buf_writer,
                    "{:016x}, {}, {:?}",
                    key.0.assemble_no_crc(),
                    key.0.opcode(),
                    value
                )
                .expect("Failed to write to output file");
            }
        }
    }
}
