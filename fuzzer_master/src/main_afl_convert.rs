//! Main AFL Converter Module
//!
//! This module provides functionality for converting AFL fuzzing findings to inputs usable with the manual execution fuzzing tool

use clap::Parser;
use itertools::Itertools;
use log::error;
use std::fs;
use std::path::PathBuf;

/// Command-line arguments for the AFL conversion tool
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Path to the AFL findings directory
    #[arg(short, long)]
    findings_path: PathBuf,
    /// Path for the output converted files
    #[arg(short, long)]
    output_path: PathBuf,
}

/// Main entry point for converting AFL findings
pub fn main() {
    env_logger::init();

    if let Err(err) = performance_timing::initialize(54_000_000.0) {
        error!("Failed to initialize performance timing: {:?}", err);
        return;
    }

    let args = Args::parse();

    if !args.findings_path.exists() {
        error!("Findings path does not exist: {:?}", args.findings_path);
        return;
    }
    if !args.output_path.exists() {
        if let Err(err) = fs::create_dir(&args.output_path) {
            error!("Failed to create output directory: {:?}", err);
            return;
        }
    }
    if !args.findings_path.is_dir() {
        error!("Findings path is not a directory: {:?}", args.findings_path);
        return;
    }
    if !args.output_path.is_dir() {
        error!("Output path is not a directory: {:?}", args.output_path);
        return;
    }

    for entry in fs::read_dir(&args.findings_path).expect("Failed to read findings directory") {
        let entry = entry.expect("Failed to read entry");
        if entry
            .file_type()
            .expect("Failed to get file type")
            .is_file()
        {
            let contents = fs::read(&entry.path()).expect("Failed to read file");
            let output_file = args.output_path.join(entry.file_name());

            let text = contents.into_iter().map(|x| format!("{x:02x}")).join(" ");

            if let Err(err) = fs::write(&output_file, text) {
                error!(
                    "Failed to write to output file {:?}: {:?}",
                    output_file, err
                );
            } else {
                println!("Converted: {:?} -> {:?}", entry.path(), output_file);
            }
        }
    }
}
