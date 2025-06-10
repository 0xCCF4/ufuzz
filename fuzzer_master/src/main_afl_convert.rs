use std::fs;
use std::path::PathBuf;
use clap::Parser;
use itertools::Itertools;
use log::error;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    findings_path: PathBuf,
    #[arg(short, long)]
    output_path: PathBuf,
}

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
        if entry.file_type().expect("Failed to get file type").is_file() {
            let contents = fs::read(&entry.path()).expect("Failed to read file");
            let output_file = args.output_path.join(entry.file_name());
            
            let text = contents.into_iter().map(|x| format!("{x:02x}")).join(" ");
            
            if let Err(err) = fs::write(&output_file, text) {
                error!("Failed to write to output file {:?}: {:?}", output_file, err);
            } else {
                println!("Converted: {:?} -> {:?}", entry.path(), output_file);
            }
        }
    }
}