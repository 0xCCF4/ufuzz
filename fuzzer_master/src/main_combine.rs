use clap::Parser;
use fuzzer_master::database::Database;
use log::error;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    inputs: Vec<PathBuf>,
    #[clap(long, short)]
    output: PathBuf,
}

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

    let databases: Vec<_> = args
        .inputs
        .iter()
        .filter_map(|input| {
            if !input.exists() {
                eprintln!("Database file does not exist: {:?}", input);
                return None;
            }

            print!("Loading database from {:?}: ", input);
            let r = match fuzzer_master::database::Database::from_file(input) {
                Ok(db) => Some(db),
                Err(e) => {
                    eprintln!("Failed to load the database {:?}: {:?}", input, e);
                    std::process::exit(1);
                }
            };
            println!("OK");
            r
        })
        .collect();

    if databases.is_empty() {
        eprintln!("No valid databases were loaded");
        return;
    }

    println!("Merging...");

    for database in databases {
        database_out.merge(database);
    }

    println!("Saving...");

    if let Err(err) = database_out.save().await {
        eprintln!("Failed to save the merged database: {:?}", err);
        std::process::exit(1);
    }
}
