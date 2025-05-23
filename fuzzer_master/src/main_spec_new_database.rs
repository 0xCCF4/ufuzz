use clap::Parser;
use fuzzer_master::spec_fuzz;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    input: PathBuf,
    #[arg(short, long)]
    output: PathBuf,
}

pub fn main() {
    env_logger::init();

    let args = Args::parse();

    spec_fuzz::copy_database_template(args.input, args.output);
}
