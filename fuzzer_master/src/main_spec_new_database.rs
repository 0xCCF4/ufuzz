//! Tools to create new speculative execution databases
//!

use clap::Parser;
use fuzzer_master::spec_fuzz;
use std::path::PathBuf;

/// Command-line arguments for the database creation tool
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Path to the input template database
    #[arg(short, long)]
    input: PathBuf,
    /// Path for the output database
    #[arg(short, long)]
    output: PathBuf,
}

/// Main entry point for creating a new speculative execution database
///
/// This function creates a new database by copying a template database to the
/// specified output location.
pub fn main() {
    env_logger::init();

    let args = Args::parse();

    spec_fuzz::copy_database_template(args.input, args.output);
}
