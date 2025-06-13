//! Module to analyze results from speculative execution fuzzing
//!
//! This module provides functionality for comparing speculative execution results from different fuzzing campaigns.

use clap::Parser;
use fuzzer_master::spec_fuzz::{SpecReport, SpecResult};
use itertools::Itertools;
use std::cmp::PartialEq;
use std::collections::BTreeMap;
use std::env;
use std::fs::File;
use std::io::BufReader;
use std::io::Write;
use std::path::PathBuf;
use ucode_compiler_dynamic::instruction::Instruction;

/// Command-line arguments for the speculative execution comparison tool
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Path for outputting unstable microcode operations to exclude from future runs
    #[arg(short, long)]
    output_unstable: Option<PathBuf>,
    /// Source files containing execution results of fuzzing campaigns
    sources: Vec<PathBuf>,
}

/// Classification of execution result stability
#[derive(Debug, PartialEq)]
pub enum ResultFlag {
    /// Result consistently timed out
    StableTimeout,
    /// Result consistently executed successfully
    StableRuns,
    /// Result showed inconsistent behavior
    Unstable,
}

/// Main entry point for comparing speculative execution results
///
/// This function loads and compares speculative execution results.
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

    println!("Numbers:");
    println!("  Stable runs: {}", count_stable_run);
    println!("  Stable timeouts: {}", count_stable_timeout);
    println!("  Unstable: {}", count_unstable);
    println!("  Total: {}", resulting_grade.len());

    if let Some(output_unstable) = args.output_unstable {
        let file = File::create(output_unstable).expect("Failed to create output file");
        let mut buf_writer = std::io::BufWriter::new(file);
        for (key, value) in resulting_grade.iter().sorted_by(|a, b| {
            a.0 .0
                .opcode()
                .to_string()
                .cmp(&b.0 .0.opcode().to_string())
        }) {
            if matches!(value, ResultFlag::Unstable | ResultFlag::StableTimeout) {
                writeln!(
                    buf_writer,
                    "{:016x}, {}, {:?}, {}",
                    key.0.assemble_no_crc(),
                    key.0.opcode(),
                    value,
                    disassemble(key.0)
                )
                .expect("Failed to write to output file");
            }
        }
    }

    println!();
    println!("Observed behaviors:");

    let mut pmc_delta_all = BTreeMap::new();
    let mut pmc_delta_per_pmc_all = BTreeMap::new();
    let mut pmc_delta_per_pmc_all_reduced = BTreeMap::new();
    let mut arch_delta_all = BTreeMap::new();

    for (instruction, flagged) in resulting_grade.iter() {
        if flagged == &ResultFlag::StableRuns {
            for run in parsed.iter() {
                if let Some(SpecResult::Executed {
                    pmc_delta,
                    arch_delta,
                }) = run.get(*instruction)
                {
                    let entry = arch_delta_all
                        .entry(instruction.0)
                        .or_insert(BTreeMap::new());
                    for v in arch_delta {
                        let entry = entry.entry(v.clone()).or_insert(0u32);
                        *entry += 1;
                    }

                    let entry = pmc_delta_all
                        .entry(instruction.0)
                        .or_insert(BTreeMap::new());
                    for (k, v) in pmc_delta {
                        if *v == 0 {
                            continue;
                        }
                        let entry = entry.entry(k.0).or_insert(BTreeMap::new());
                        let entry = entry.entry(*v).or_insert(0u32);
                        *entry += 1;

                        let entry = pmc_delta_per_pmc_all.entry(k.0).or_insert(BTreeMap::new());
                        let entry = entry.entry(v).or_insert((0u32, BTreeMap::new()));
                        entry.0 += 1;
                        *entry.1.entry(instruction.0).or_insert(0u32) += 1;

                        let entry = pmc_delta_per_pmc_all_reduced
                            .entry(k.0)
                            .or_insert(BTreeMap::new());
                        let entry = entry.entry(v.signum()).or_insert((0u32, BTreeMap::new()));
                        entry.0 += 1;
                        *entry.1.entry(instruction.0).or_insert(0u32) += 1;
                    }
                }
            }
        }
    }

    let count_instruction_with_pmc_delta =
        pmc_delta_all.iter().filter(|(_, v)| !v.is_empty()).count();
    let count_instruction_with_arch_delta =
        arch_delta_all.iter().filter(|(_, v)| !v.is_empty()).count();

    println!(
        "  Instructions with PMC delta: {}",
        count_instruction_with_pmc_delta
    );
    for pmc in pmc_delta_per_pmc_all_reduced
        .iter()
        .filter(|(_, v)| !v.is_empty())
    {
        for (key, val) in pmc.1.iter() {
            let mut by_opcode = BTreeMap::new();
            for (k, v) in val.1.iter() {
                let entry = by_opcode.entry(k.opcode()).or_insert(0u32);
                *entry += *v;
            }
            println!(
                "    {:02x}:{:02x}, {key:>2}:{:>5}, {:?}...",
                pmc.0.event_select,
                pmc.0.umask,
                val.0,
                by_opcode
                    .iter()
                    .sorted_by(|a, b| a.1.cmp(b.1))
                    .rev()
                    .take(4)
                    .collect_vec()
            );
        }
    }
    println!(
        "  Instructions with arch delta: {}",
        count_instruction_with_arch_delta
    );
    for instruction in arch_delta_all.iter().filter(|(_, v)| !v.is_empty()) {
        println!(
            "    {:016x}, {}, {:?}, {}",
            instruction.0.assemble_no_crc(),
            instruction.0.opcode(),
            instruction.1,
            disassemble(*instruction.0)
        );
    }
}

/// Disassembles an microcode operation using the UASM tool
///
/// # Arguments
///
/// * `instruction` - microcode operation to disassemble
///
/// # Returns
///
/// A string containing the disassembled instruction
fn disassemble(instruction: Instruction) -> String {
    let env_uasm_dis = env::var("UASM");

    if let Ok(uasm_dis) = env_uasm_dis {
        let mut cmd = std::process::Command::new("python3");
        cmd.arg(uasm_dis);
        cmd.arg("-d");
        cmd.arg("-u");
        cmd.arg(format!("0x{:016x}", instruction.assemble_no_crc()));
        let output = cmd.output().expect("Failed to run uasm");
        if output.status.success() {
            String::from_utf8_lossy(&output.stdout)
                .to_string()
                .lines()
                .skip(6)
                .next()
                .map(|x| x.to_string())
                .unwrap_or(String::new())
        } else {
            println!("Failed to run uasm: {:?}", output);
            "".to_string()
        }
    } else {
        "".to_string()
    }
}
