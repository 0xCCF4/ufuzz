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
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

mod bochs;

type DynError = Box<dyn std::error::Error>;

#[derive(Parser)]
#[command(author, about, long_about = None)]
enum Cli {
    /// Emulate a UEFI application using BOCHS CPU emulator
    Emulate(EmulateCli),
    /// Push an UEFI app onto the remote machine
    PutRemote {
        /// name of the target
        name: String,
        /// override UEFI startup
        #[clap(short, long)]
        startup: bool,
        /// Release mode
        #[clap(short, long)]
        release: bool,
    },
    /// Control a remote machine
    ControlRemote {
        /// Arguments to pass to the remote machine
        args: Vec<String>,
    },
    /// Update the node's software, systemd service, etc
    UpdateNode,
    /// Generate documentation
    Doc,
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
        Cli::PutRemote {
            name,
            startup,
            release,
        } => main_put_remote(&name, startup, release),
        Cli::ControlRemote { args } => main_control_remote(args),
        Cli::Doc => main_generate_doc(),
    }
}

fn main_control_remote(args: Vec<String>) {
    let host_node = env::var("HOST_NODE").unwrap_or("10.0.0.10".to_string());
    let ssh_args = match env::var("SSH_ARGS") {
        Ok(args) => args
            .split_whitespace()
            .clone()
            .into_iter()
            .map(str::to_string)
            .collect(),
        Err(_) => Vec::new(),
    };
    let mut ssh = Command::new("ssh")
        .args(ssh_args)
        .arg(format!("thesis@{host_node}").as_str())
        .stdin(Stdio::piped())
        .spawn()
        .expect("Failed to start ssh");

    {
        let ssh_stdin = ssh.stdin.as_mut().expect("Failed to open ssh stdin");
        write!(ssh_stdin, "sudo /run/current-system/sw/bin/device_control ")
            .expect("Failed to write to ssh stdin");
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

fn main_put_remote(name: &str, startup: bool, release: bool) {
    let ssh_args = match env::var("SSH_ARGS") {
        Ok(args) => args
            .split_whitespace()
            .clone()
            .into_iter()
            .map(str::to_string)
            .collect(),
        Err(_) => Vec::new(),
    };
    let sftp_args = match env::var("SFTP_ARGS") {
        Ok(args) => args
            .split_whitespace()
            .clone()
            .into_iter()
            .map(str::to_string)
            .collect(),
        Err(_) => Vec::new(),
    };
    let (name, path) = if name == "corpus" {
        let project_root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
        let project_root = project_root.parent().unwrap().to_path_buf();
        let mut gen = Command::new("distrobox-host-exec")
            .arg("nix")
            .arg("build")
            .current_dir(project_root.join("corpus-gen"))
            .spawn()
            .expect("Failed to spawn 'nix build' process");

        let status = gen.wait().expect("Failed to wait on 'nix build' process");
        if !status.success() {
            eprintln!("Failed to build the corpus generator");
            std::process::exit(-1);
        }

        let mut gen = Command::new("cargo")
            .arg("run")
            .arg("--locked")
            .arg("-p")
            .arg("corpus-gen")
            .arg("--")
            .arg("result/lib")
            .arg("corpus.json")
            .current_dir(project_root.join("corpus-gen"))
            .spawn()
            .expect("Failed to spawn 'corpus gen' process");

        let status = gen.wait().expect("Failed to wait on 'corpus gen' process");
        if !status.success() {
            eprintln!("Failed to generate the corpus");
            std::process::exit(-1);
        }

        let corpus = project_root.join("corpus-gen").join("corpus.json");

        (
            "corpus.json".to_string(),
            corpus.as_os_str().to_str().unwrap().to_string(),
        )
    } else {
        let app = build_app(name, release, true).expect("Failed to build the app");
        let name = app
            .file_name()
            .expect("Failed to get the app file name")
            .to_str()
            .unwrap()
            .to_string();

        (name, app.as_os_str().to_str().unwrap().to_string())
    };

    let host_node = env::var("HOST_NODE").unwrap_or("10.0.0.10".to_string());

    println!("Pushing {} to the remote", name);
    let mut sftp = Command::new("sftp")
        .args(sftp_args)
        .arg(&format!("thesis@{}:/tmp", host_node.as_str()))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .expect("Failed to start sftp");

    let sftp_stdin = sftp.stdin.as_mut().expect("Failed to open sftp stdin");
    writeln!(sftp_stdin, "put {}", path).expect("Failed to write to sftp stdin");
    writeln!(sftp_stdin, "exit").expect("Failed to write to sftp stdin");

    let _status = sftp.wait().expect("Failed to wait on sftp");

    let mut ssh = Command::new("ssh")
        .args(ssh_args)
        .arg(&format!("thesis@{}", host_node.as_str()))
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
        writeln!(ssh_stdin, "if [ ! -f ~/disk.img ]; then echo 'Creating disk image' && dd if=/dev/zero of=~/disk.img bs=1M count=8192 && echo 'Formatting disk image' && mkfs.fat -F 32 ~/disk.img; fi").expect("Failed to write to ssh stdin");
        writeln!(ssh_stdin, "sudo mkdir -p /mnt ; sudo mount ~/disk.img /mnt")
            .expect("Failed to write to ssh stdin");
        writeln!(ssh_stdin, "sudo cp \"/tmp/{}\" \"/mnt/{}\"", name, name)
            .expect("Failed to write to ssh stdin");

        if name != "corpus.json" && startup {
            writeln!(ssh_stdin, "echo \"{}\" | sudo tee /mnt/startup.nsh", name)
                .expect("Failed to write to ssh stdin");
        }

        writeln!(ssh_stdin, "sudo umount /mnt").expect("Failed to write to ssh stdin");
        writeln!(ssh_stdin, "sudo rm \"/tmp/{}\"", name).expect("Failed to write to ssh stdin");
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
        .args(["nix", "run"])
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

    let _target_file = project_directory
        .parent()
        .unwrap()
        .join("uefi-target-with-debug.json");

    status.args([
        "build",
        "--locked",
        "-Zbuild-std",
        "--target",
        if device {
            "x86_64-unknown-uefi"
        } else {
            //target_file.to_str().unwrap()
            "x86_64-unknown-uefi"
        },
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
    } else if project == "spec_fuzz" {
        status.args(["-p", project, "--bins"]);
        "spec_fuzz"
    } else if project == "hypervisor" {
        status.args(["-p", project, "--example", "test_hypervisor"]);
        "examples/test_hypervisor"
    } else if project == "test_time" {
        status.args([
            "-p",
            "fuzzer_device",
            "--example",
            "test_time",
            "--features",
            "device_bochs,mutation_all",
            "--no-default-features",
        ]);
        "examples/test_time"
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
    } else if project == "test_setup" {
        status.args([
            "-p",
            "coverage",
            "--example",
            "test_setup_arged",
            "--features",
            "uefi",
        ]);
        "examples/test_setup_arged"
    } else if project == "test_rdrand" {
        status.args([
            "-p",
            "coverage",
            "--example",
            "test_rdrand",
            "--features",
            "uefi",
        ]);
        "examples/test_rdrand"
    } else if project == "test_exp_hook_odd" {
        status.args([
            "-p",
            "coverage",
            "--example",
            "test_exp_hook_odd",
            "--features",
            "uefi",
        ]);
        "examples/test_exp_hook_odd"
    } else if project == "test_exp_hook_even" {
        status.args([
            "-p",
            "coverage",
            "--example",
            "test_exp_hook_even",
            "--features",
            "uefi",
        ]);
        "examples/test_exp_hook_even"
    } else if project == "speculation_x86" {
        status.args(["-p", "speculation_x86"]);
        "speculation_x86"
    } else if project == "speculation_ucode" {
        status.args(["-p", "speculation_ucode"]);
        "speculation_ucode"
    } else if project == "speculation_ucode_hypervisor" {
        status.args([
            "-p",
            "fuzzer_device",
            "--example",
            "test_ucode_speculation",
            "--features",
            "device_bochs,mutation_all",
            "--no-default-features",
        ]);
        "examples/test_ucode_speculation"
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

    let target_folder = if device {
        build_folder.join("x86_64-unknown-uefi")
    } else {
        build_folder.join("x86_64-unknown-uefi")
    };

    match status.status() {
        Ok(status) if !status.success() => Err("Failed to build the hypervisor")?,
        Err(_) => Err("Failed to build the hypervisor")?,
        Ok(_) => Ok(target_folder
            .join(if release { "release" } else { "debug" })
            .join(format!("{}.efi", executable_name))),
    }
}

fn main_generate_doc() {
    let project_root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    let projects_uefi = [
        "coverage",
        "custom_processing_unit",
        "fuzzer_device",
        "hypervisor",
        "spec_fuzz",
        "speculation_x86",
        "speculation_ucode",
        "uefi_udp4",
    ];
    let projects_x86 = [
        "corpus-gen",
        "data_types",
        "fuzzer_data",
        "fuzzer_master",
        "fuzzer_node",
        "literature_search",
        "performance_timing",
        "performance_timing_macros",
        "ucode_compiler_bridge",
        "ucode_compiler_derive",
        "ucode_compiler_dynamic",
        "ucode_dump",
        "x86_perf_counter",
    ];

    fn cmd(target: Option<&str>, safe: bool) -> Command {
        let mut cmd = Command::new("cargo");
        cmd.arg("doc")
            .env("RUSTFLAGS", "-D warnings")
            .arg("--locked")
            .arg("--no-deps")
            .arg("--document-private-items");
        if let Some(target) = target {
            cmd.arg("--target").arg(target);
        }
        if safe {
            cmd.arg("-j2");
        }
        cmd
    }

    for target in ["x86_64-unknown-uefi", "x86_64-unknown-linux-gnu"] {
        // Create a symlink from target/x86_64-unknown-linux-gnu/doc to target/doc
        let target_doc = project_root
            .parent()
            .unwrap()
            .join("target")
            .join(target)
            .join("doc");
        let common_doc = project_root.parent().unwrap().join("target").join("doc");
        if target_doc.exists() {
            if target_doc.is_dir() {
                fs::remove_dir_all(&target_doc).expect("Failed to remove existing doc directory");
            } else {
                fs::remove_file(&target_doc).expect("Failed to remove existing doc symlink/file");
            }
        }
        if !common_doc.exists() {
            fs::create_dir(&common_doc).expect("Failed to create doc symlink directory");
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(&common_doc, &target_doc)
            .expect("Failed to create symlink for doc");
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&common_doc, &target_doc)
            .expect("Failed to create symlink for doc");
    }

    for project in projects_uefi {
        let status = cmd(Some("x86_64-unknown-uefi"), false)
            .arg("-p")
            .arg(project)
            .current_dir(&project_root)
            .status()
            .expect("Failed to generate documentation");
        if !status.success() {
            eprintln!("Failed to generate documentation for {project}");
            std::process::exit(-1);
        }
    }

    for project in projects_x86 {
        let status = cmd(
            Some("x86_64-unknown-linux-gnu"),
            ["fuzzer_master"].contains(&project),
        )
        .arg("-p")
        .arg(project)
        .current_dir(&project_root)
        .status()
        .expect("Failed to generate documentation");
        if !status.success() {
            eprintln!("Failed to generate documentation for {project}");
            std::process::exit(-1);
        }
    }

    if !cmd(None, false)
        .env("RUSTDOCFLAGS", "--enable-index-page -Zunstable-options")
        .args(["-p", "xtask"])
        .current_dir(&project_root)
        .status()
        .expect("Failed to generate documentation for xtask")
        .success()
    {
        eprintln!("Failed to generate documentation for xtask");
        std::process::exit(-1);
    }
}
