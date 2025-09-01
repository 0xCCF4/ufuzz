//! Tools to combine fuzzing databases
//!
//! This module provides functionality for combining multiple fuzzing databases.

use clap::Parser;
use flate2::Compression;
use fuzzer_master::database::Database;
use log::error;
use std::path::PathBuf;
use tokio::sync::mpsc::unbounded_channel;

/// Command-line arguments for the database combination tool
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Compression level (0-9)
    #[clap(long, default_value_t = 6)]
    compression: u32,
    /// Input database files to combine
    inputs: Vec<PathBuf>,
    /// Path for the output combined database
    #[clap(long, short)]
    output: PathBuf,
}

/// Main entry point for combining fuzzing databases
///
/// This function combines multiple input databases into a single output database,
/// handling validation and error cases.
#[tokio::main]
async fn main() {
    env_logger::init();

    if let Err(err) = performance_timing::initialize(54_000_000.0) {
        error!("Failed to initialize performance timing: {:?}", err);
        return;
    }

    let args = Args::parse();

    let mut database_out = match Database::from_file(&args.output) {
        Ok(_db) => {
            eprintln!("Output database already exists!");
            std::process::exit(1);
        }
        Err(_e) => Database::empty(&args.output),
    };

    let (sender, mut receiver) = unbounded_channel();

    let databases_readers: Vec<_> = args
        .inputs
        .iter()
        .map(|input| {
            let sender_copy = sender.clone();
            let input_clone = input.clone();
            tokio::task::spawn_blocking(move || {
                if !input_clone.exists() {
                    eprintln!("Database file does not exist: {:?}", input_clone);
                    std::process::exit(1);
                }

                let r = match fuzzer_master::database::Database::from_file(&input_clone) {
                    Ok(db) => Some(db),
                    Err(e) => {
                        eprintln!("Failed to load the database {:?}: {:?}", input_clone, e);
                        std::process::exit(1);
                    }
                };
                println!("Loaded database from {:?} ", input_clone);

                if let Some(db) = r {
                    if let Err(err) = sender_copy.send(db) {
                        eprintln!("Failed to send database from thread: {:?}", err);
                        std::process::exit(1);
                    }
                }
            })
        })
        .collect();

    drop(sender);

    let number_of_databases = databases_readers.len();

    let merge = tokio::spawn(async move {
        let mut database = None;
        let mut counter = 0;
        while let Some(db) = receiver.recv().await {
            let path = db.path.clone();
            match database {
                None => database = Some(db),
                Some(ref mut existing_db) => existing_db.merge(db),
            }
            counter += 1;
            println!(
                "Merged database ({counter}/{number_of_databases}) with {:?}",
                path
            );
        }
        println!("All databases merged.");
        database
    });

    for reader in databases_readers {
        if let Err(err) = reader.await {
            eprintln!("Failed to join database reader task: {:?}", err);
            std::process::exit(1);
        }
    }

    let merge = merge.await;

    let merge = match merge {
        Err(err) => {
            eprintln!("Failed to join merge task: {:?}", err);
            std::process::exit(1);
        }
        Ok(db) => db,
    };

    match merge {
        None => {
            eprintln!("No databases were merged");
            std::process::exit(1);
        }
        Some(mut db) => {
            db.compression = database_out.compression;
            db.path = database_out.path.clone();
            database_out = db;
        }
    }

    println!("Saving...");

    database_out.compression = Compression::new(args.compression);
    if let Err(err) = database_out.save_progress(true).await {
        eprintln!("Failed to save the merged database: {:?}", err);
        std::process::exit(1);
    }
}
