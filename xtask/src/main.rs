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

    /// External telnet
    #[arg(short, long)]
    port: Option<u16>,

    /// The project name
    project: String,

    /// The subcommand to run
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
        Commands::BochsIntel => start_vm(
            Bochs {
                cpu: Cpu::Intel,
                port: cli.port,
            },
            cli.release,
            &cli.project,
        ),
        Commands::BochsAmd => start_vm(
            Bochs {
                cpu: Cpu::Amd,
                port: cli.port,
            },
            cli.release,
            &cli.project,
        ),
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

fn start_vm<T: VM>(vm: T, release: bool, project: &str) -> Result<(), DynError> {
    let build_output = build_app(project, release)?;
    let project_root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    let env = vm.deploy("/tmp", project_root.join("config"), build_output)?;
    vm.run(env)?;

    Ok(())
}

fn build_app(project: &str, release: bool) -> Result<PathBuf, DynError> {
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());

    let mut status = Command::new(cargo);

    status.args(["build", "-p", project, "--target", "x86_64-unknown-uefi"]);

    let executable_name = if project == "fuzzer_device" {
        status.args([
            "--bins",
            "--features",
            "bios_bochs,platform_bochs,rand_isaac,mutation_all",
            "--no-default-features",
        ]);
        "fuzzer_device"
    } else if project == "hypervisor" {
        status.args(["--example", "test_hypervisor"]);
        "examples/test_hypervisor"
    } else {
        project
    };

    if release {
        status.args(["--release"]);
    }

    println!("{:?}", status);

    let build_folder = env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| "xtask".to_string()))
                .parent()
                .unwrap()
                .join("target")
        });

    match status.status() {
        Ok(status) if !status.success() => Err("Failed to build the hypervisor")?,
        Err(_) => Err("Failed to build the hypervisor")?,
        Ok(_) => Ok(build_folder
            .join("x86_64-unknown-uefi")
            .join("debug")
            .join(format!("{}.efi", executable_name))),
    }
}
