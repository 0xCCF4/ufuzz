//! Main fuzzing output comapre module
//!
//! This module provides functionality for comparing execution results from different manual fuzzing input execution.

use clap::Parser;
use fuzzer_data::ExecutionResult;
use hypervisor::state::StateDifference;
use itertools::Itertools;
use std::path::PathBuf;

/// Arguments for the compare tool
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Source files containing fuzzing input results to compare
    sources: Vec<PathBuf>,
}

/// Main entry point for comparing execution results
///
/// This function reads and compares execution results from two source files,
/// analyzing differences in exit codes, coverage, and state.
pub fn main() {
    env_logger::init();

    let args = Args::parse();

    if args.sources.len() != 2 {
        println!("Expected 2 sources, got {}", args.sources.len());
        std::process::exit(1);
    }

    let text_r = args
        .sources
        .iter()
        .map(std::fs::read_to_string)
        .collect_vec();
    let mut text = Vec::with_capacity(text_r.len());
    for (result, source) in text_r.into_iter().zip(args.sources.iter()) {
        if let Err(e) = result {
            println!("Failed to read file {:?}: {}", source, e);
            std::process::exit(1);
        }
        text.push(result.unwrap());
    }

    let exits: Vec<ExecutionResult> = text
        .iter()
        .map(|source| {
            let start = source.find("Exit result:\n").unwrap();
            let end = source.find("END-EXIT\n").unwrap();
            let out = &source[start + 13..end];
            serde_json::from_str(out).unwrap()
        })
        .collect_vec();

    let exit1 = &exits[0];
    let exit2 = &exits[1];

    if exit1.exit != exit2.exit {
        println!("Different exit codes:");
        println!("  Source 1: {:#x?}", exit1.exit);
        println!("  Source 2: {:#x?}", exit2.exit);
    } else {
        println!("- same exit - ");
    }

    if exit1.coverage != exit2.coverage {
        println!("Different coverage");
    } else {
        println!("- same coverage - ");
    }

    if exit1.state != exit2.state {
        println!("Different state");
        for (name, a, b) in exit1.state.difference(&exit2.state) {
            println!(" - {name} {a:x?} -> {b:x?}");
        }
    } else {
        println!("- same state - ");
    }
}
