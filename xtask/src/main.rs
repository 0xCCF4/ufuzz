//! A build and test assist program. To show the usage, run
//!
//! ```shell
//! cargo xtask
//! ```

#![allow(clippy::multiple_crate_versions)]

use bochs::{Bochs, Cpu};
use clap::{Parser, Subcommand};
use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

mod bochs;

type DynError = Box<dyn std::error::Error>;

#[derive(Parser)]
#[command(author, about, long_about = None)]
struct Cli {
    /// Build the hypervisor with the release profile
    #[arg(short, long)]
    release: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start a Bochs VM with an Intel processor
    BochsIntel,
    /// Start a Bochs VM with an AMD processor
    BochsAmd,
}

fn main() {
    let cli = Cli::parse();
    let result = match &cli.command {
        Commands::BochsIntel => start_vm(Bochs { cpu: Cpu::Intel }, cli.release),
        Commands::BochsAmd => start_vm(Bochs { cpu: Cpu::Amd }, cli.release),
    };
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(-1);
    }
}

trait VM {
    type T;
    fn deploy<A: AsRef<Path>, B: AsRef<Path>, C: AsRef<Path>>(
        &self,
        working_directory: A,
        config_directory: B,
        build_output: C,
    ) -> Result<Self::T, DynError>;
    fn run(&self, environment: Self::T) -> Result<(), DynError>;
}

fn start_vm<T: VM>(vm: T, release: bool) -> Result<(), DynError> {
    let build_output = build_hypervisor_app(release)?;
    let project_root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    let env = vm.deploy("/tmp", project_root.join("config"), build_output)?;
    vm.run(env)?;

    Ok(())
}

fn build_hypervisor_app(release: bool) -> Result<PathBuf, DynError> {
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());

    let mut status = Command::new(cargo);

    status.args([
        "build",
        "--bin",
        "hypervisor",
        "--target",
        "x86_64-unknown-uefi",
    ]);

    if release {
        status.args(["--release"]);
    }

    match status.status() {
        Ok(status) if !status.success() => Err("Failed to build the hypervisor")?,
        Err(_) => Err("Failed to build the hypervisor")?,
        Ok(_) => Ok(PathBuf::from(
            "target/x86_64-unknown-uefi/debug/hypervisor.efi",
        )),
    }
}
