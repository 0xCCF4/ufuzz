//! A build and test assist program. To show the usage, run
//!
//! ```shell
//! cargo xtask
//! ```

#![allow(clippy::multiple_crate_versions)]

use bochs::{Bochs, Cpu};
use clap::{Parser, Subcommand};
use std::io::Write;
use std::process::Stdio;
use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

mod bochs;

type DynError = Box<dyn std::error::Error>;

#[derive(Parser)]
#[command(author, about, long_about = None)]
enum Cli {
    Emulate(EmulateCli),
    PutRemote {
        /// name of the target
        name: String,
    },
    ControlRemote {
        args: Vec<String>,
    },
    UpdateNode,
}

#[derive(Parser)]
struct EmulateCli {
    /// Build the hypervisor with the release profile
    #[arg(short, long)]
    release: bool,

    /// External telnet
    #[arg(short, long)]
    port: Option<u16>,

    /// The project name
    project: String,

    #[arg(short, long, default_value = "false")]
    ctrlc: bool,

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
    match cli {
        Cli::Emulate(cli) => main_run(cli),
        Cli::UpdateNode => main_update_node(),
        Cli::PutRemote { name } => main_put_remote(&name),
        Cli::ControlRemote { args } => main_control_remote(args),
    }
}

fn main_control_remote(args: Vec<String>) {
    let mut ssh = Command::new("ssh")
        .arg("thesis@192.168.0.6")
        .stdin(Stdio::piped())
        .spawn()
        .expect("Failed to start ssh");

    {
        let ssh_stdin = ssh.stdin.as_mut().expect("Failed to open ssh stdin");
        writeln!(ssh_stdin, "device_control").expect("Failed to write to ssh stdin");
        for arg in args {
            write!(ssh_stdin, "{:?} ", arg).expect("Failed to write to ssh stdin");
        }
        writeln!(ssh_stdin).expect("Failed to write to ssh stdin");
        writeln!(ssh_stdin, "exit").expect("Failed to write to ssh stdin");
    }

    let status = ssh.wait().expect("Failed to wait on ssh");

    if !status.success() {
        eprintln!("Failed to execute remote commands");
        std::process::exit(-1);
    }
}

fn main_put_remote(name: &str) {
    let app = build_app(name, false, true).expect("Failed to build the app");
    let name = app
        .file_name()
        .expect("Failed to get the app file name")
        .to_str()
        .unwrap();

    println!("Pushing {} to the remote", name);
    let mut sftp = Command::new("sftp")
        .arg("thesis@192.168.0.6:/tmp")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .expect("Failed to start sftp");

    let sftp_stdin = sftp.stdin.as_mut().expect("Failed to open sftp stdin");
    writeln!(sftp_stdin, "put {}", app.as_os_str().to_str().unwrap())
        .expect("Failed to write to sftp stdin");
    writeln!(sftp_stdin, "exit").expect("Failed to write to sftp stdin");

    let status = sftp.wait().expect("Failed to wait on sftp");

    let mut ssh = Command::new("ssh")
        .arg("thesis@192.168.0.6")
        .stdin(Stdio::piped())
        .spawn()
        .expect("Failed to start ssh");

    {
        let ssh_stdin = ssh.stdin.as_mut().expect("Failed to open ssh stdin");
        writeln!(ssh_stdin, "echo Syncing && sudo sync").expect("Failed to write to ssh stdin");
        writeln!(
            ssh_stdin,
            "echo -n \"Stopping USB: \" && sudo configure_usb off"
        )
        .expect("Failed to write to ssh stdin");
        writeln!(ssh_stdin, "if [ ! -f ~/disk.img ]; then echo 'Creating disk image' && dd if=/dev/zero of=~/disk.img bs=1M count=1024 && echo 'Formatting disk image' && mkfs.fat -F 32 ~/disk.img; fi").expect("Failed to write to ssh stdin");
        writeln!(ssh_stdin, "sudo mkdir -p /mnt ; sudo mount ~/disk.img /mnt")
            .expect("Failed to write to ssh stdin");
        writeln!(ssh_stdin, "sudo cp \"/tmp/{}\" /mnt/{}", name, name)
            .expect("Failed to write to ssh stdin");
        writeln!(ssh_stdin, "sudo umount /mnt").expect("Failed to write to ssh stdin");
        writeln!(ssh_stdin, "sudo rm /tmp/{}", name).expect("Failed to write to ssh stdin");
        writeln!(ssh_stdin, "echo Syncing && sudo sync").expect("Failed to write to ssh stdin");
        writeln!(
            ssh_stdin,
            "echo -n \"Starting USB: \" && sudo configure_usb on ~/disk.img"
        )
        .expect("Failed to write to ssh stdin");
        writeln!(ssh_stdin, "exit").expect("Failed to write to ssh stdin");
    }

    let status = ssh.wait().expect("Failed to wait on ssh");

    if !status.success() {
        eprintln!("Failed to execute remote commands");
        std::process::exit(-1);
    }
}

fn main_update_node() {
    let project_root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    let mut status = Command::new("distrobox-host-exec");

    status
        .args([
            "nixos-rebuild",
            "switch",
            "--use-remote-sudo",
            "--flake",
            ".#node",
            "--target-host",
            "thesis@192.168.0.6",
        ])
        .current_dir(project_root.parent().unwrap().join("nix"));

    let status = status.status().expect("Failed to update the node");

    if !status.success() {
        eprintln!("Failed to update the node");
        std::process::exit(-1);
    }
}

fn main_run(cli: EmulateCli) {
    let result = match &cli.command {
        Commands::BochsIntel => start_vm(
            Bochs {
                cpu: Cpu::Intel,
                port: cli.port,
            },
            cli.release,
            &cli.project,
            !cli.ctrlc,
        ),
        Commands::BochsAmd => start_vm(
            Bochs {
                cpu: Cpu::Amd,
                port: cli.port,
            },
            cli.release,
            &cli.project,
            !cli.ctrlc,
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
    fn run(&self, environment: Self::T, ctrlc: bool) -> Result<(), DynError>;
}

fn start_vm<T: VM>(vm: T, release: bool, project: &str, ctrlc: bool) -> Result<(), DynError> {
    let build_output = build_app(project, release, false)?;
    let project_root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    let env = vm.deploy("/tmp", project_root.join("config"), build_output)?;
    vm.run(env, ctrlc)?;

    Ok(())
}

fn build_app(project: &str, release: bool, device: bool) -> Result<PathBuf, DynError> {
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());

    let mut status = Command::new(cargo);

    let project_directory = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    let target_file = project_directory
        .parent()
        .unwrap()
        .join("uefi-target-with-debug.json");

    status.args([
        "build",
        "-Zbuild-std",
        "--target",
        target_file.to_str().unwrap(),
    ]);

    let executable_name = if project == "fuzzer_device" {
        status.args([
            "-p",
            project,
            "--bins",
            "--features",
            if device {
                "__device_brix"
            } else {
                "__device_bochs"
            },
            "--no-default-features",
        ]);
        "fuzzer_device"
    } else if project == "hypervisor" {
        status.args(["-p", project, "--example", "test_hypervisor"]);
        "examples/test_hypervisor"
    } else if project == "cmos_test" {
        status.args([
            "-p",
            "fuzzer_device",
            "--example",
            "test_cmos",
            "--features",
            "device_bochs,mutation_all",
            "--no-default-features",
        ]);
        "examples/test_cmos"
    } else if project == "test_udp" {
        status.args([
            "-p",
            "fuzzer_device",
            "--example",
            "test_udp",
            "--features",
            "device_bochs,mutation_all",
            "--no-default-features",
        ]);
        "examples/test_udp"
    } else if project == "test_hypervisor" {
        status.args(["-p", "hypervisor", "--example", "test_hypervisor"]);
        "examples/test_hypervisor"
    } else {
        status.args(["-p", project]);
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
            .join("uefi-target-with-debug")
            .join("debug")
            .join(format!("{}.efi", executable_name))),
    }
}
