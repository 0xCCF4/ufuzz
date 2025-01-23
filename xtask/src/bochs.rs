use crate::{DynError, VM};
use rand::Rng;
use std::path::PathBuf;
use std::{
    env, fmt,
    io::{BufRead, BufReader},
    path::Path,
    process::{Command, Stdio},
    sync::mpsc::channel,
    thread,
    time::{Duration, SystemTime},
};

pub(crate) struct Bochs {
    pub(crate) cpu: Cpu,
    pub(crate) port: Option<u16>,
}

pub struct BochsEnviroment {
    working_directory: PathBuf,
    port: u16,
}

impl VM for Bochs {
    type T = BochsEnviroment;

    fn deploy<A: AsRef<Path>, B: AsRef<Path>, C: AsRef<Path>>(
        &self,
        working_directory: A,
        config_directory: B,
        build_output: C,
    ) -> Result<BochsEnviroment, DynError> {
        // copy disk to working directory

        let disk = working_directory.as_ref().join("bochs_disk.img");

        if !Command::new("dd")
            .args([
                "if=/dev/zero",
                &format!("of={}", disk.to_str().unwrap()),
                "bs=1M",
                "count=64",
            ])
            .status()
            .map_err(|_| "dd failed")?
            .success()
        {
            return Err("dd failed".into());
        }

        // Create a GPT partition table and a FAT32 partition
        /*if !Command::new("parted")
            .args([disk.to_str().unwrap(), "--script", "mktable", "gpt", "mkpart", "primary", "fat32", "2048s", "100%", "align-check", "optimal", "1", "name", "1", "UEFI", "quit"])
            .status()
            .map_err(|_| "parted failed")?
            .success() {
            return Err("parted failed".into());
        }*/

        // Format the partition as FAT32
        if !Command::new("mkfs.fat")
            .args([disk.to_str().unwrap(), "-F", "32"])
            .status()
            .map_err(|_| "mkfs.vfat failed")?
            .success()
        {
            return Err("mkfs.vfat failed".into());
        }

        // Copy startup.nsh to the FAT32 partition
        if !Command::new("mcopy")
            .args([
                "-i",
                disk.to_str().unwrap(),
                &format!(
                    "{}/startup.nsh",
                    config_directory.as_ref().to_str().unwrap()
                ),
                "::",
            ])
            .status()
            .map_err(|_| "mcopy startup.nsh failed")?
            .success()
        {
            return Err("mcopy startup.nsh failed".into());
        }

        // Copy hypervisor.efi to the FAT32 partition
        if !Command::new("mcopy")
            .args([
                "-i",
                disk.to_str().unwrap(),
                build_output.as_ref().to_str().unwrap(),
                "::",
            ])
            .status()
            .map_err(|_| "mcopy hypervisor.efi failed")?
            .success()
        {
            return Err("mcopy hypervisor.efi failed".into());
        }

        let port = self
            .port
            .unwrap_or(rand::thread_rng().gen_range(1024..65535));

        let data = config_directory
            .as_ref()
            .join(format!("{}.bxrc", env::consts::OS));
        let data = std::fs::read_to_string(data).map_err(|_| "read bxrc failed")?;
        let data = data
            .replace(
                "{CPU}",
                match self.cpu.to_string().to_lowercase().as_str() {
                    "intel" => "tigerlake",
                    "amd" => "ryzen",
                    _ => panic!("unknown cpu"),
                },
            )
            .replace("{PORT}", port.to_string().as_str());

        let config = working_directory.as_ref().join("config.bxrc");

        std::fs::write(&config, data).map_err(|_| "write bxrc failed")?;

        // copy bios to working directory
        if !Command::new("cp")
            .args([
                "-r",
                config_directory.as_ref().join("bios").to_str().unwrap(),
                working_directory.as_ref().to_str().unwrap(),
            ])
            .status()
            .map_err(|_| "cp bios failed")?
            .success()
        {
            return Err("cp bios failed".into());
        }

        Ok(BochsEnviroment {
            // config_directory: config_directory.as_ref().to_path_buf(),
            working_directory: working_directory.as_ref().to_path_buf(),
            port,
        })
    }

    fn run(&self, environment: Self::T) -> Result<(), DynError> {
        // Start a threads that tries to connect to Bochs in an infinite loop.
        let port = environment.port;
        if self.port.is_none() {
            let _unused = thread::spawn(move || loop {
                thread::sleep(Duration::from_secs(4));

                let output = Command::new("telnet")
                    .args(["localhost", port.to_string().as_str()])
                    .stdout(Stdio::piped())
                    .stdin(Stdio::piped())
                    .spawn()
                    .expect("telnet connection");

                let now = SystemTime::now();

                let reader = BufReader::new(output.stdout.unwrap());
                reader.lines().map_while(Result::ok).for_each(|line| {
                    println!(
                        "{:>4}: {line}\r",
                        now.elapsed().unwrap_or_default().as_secs()
                    );
                });

                thread::sleep(Duration::from_secs(1));
            });
        }

        let (tx, rx) = channel();
        let tx2 = tx.clone();

        let (bochs_cmd_sender, bochs_cmd_receiver) = channel();

        let bochs = thread::spawn(move || {
            let bxrc =
                std::fs::canonicalize(environment.working_directory.join("config.bxrc")).unwrap();

            let bochs = std::env::var("BOCHS").unwrap_or("bochs".to_string());

            let mut output = Command::new(bochs);
            output
                .args([
                    "-q",
                    "-unlock",
                    /*"-rc",
                    std::fs::canonicalize(environment.config_directory.join("dbg_command.txt"))
                        .unwrap()
                        .to_str()
                        .unwrap(),*/
                    "-f",
                    &bxrc.to_str().unwrap(),
                ])
                .current_dir(environment.working_directory)
                .stdout(Stdio::piped());

            println!("Starting bochs with command: {:?}", output);

            let output = output.spawn().expect("bochs run failed");

            bochs_cmd_sender.send(output.id()).unwrap();

            // Read and print stdout as they come in. This does not return.
            let reader = BufReader::new(output.stdout.unwrap());
            reader
                .lines()
                .map_while(Result::ok)
                .for_each(|line| println!("{line}\r"));

            tx2.send(()).unwrap();
        });

        ctrlc::set_handler(move || tx.send(()).unwrap())?;

        rx.recv()?;

        thread::sleep(Duration::from_secs(1));

        let bochs_id = bochs_cmd_receiver.recv().unwrap();
        Command::new("kill")
            .args(["-9", &bochs_id.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("kill bochs failed");

        bochs.join().unwrap();

        Ok(())
    }
}

#[derive(Debug)]
pub(crate) enum Cpu {
    Intel,
    Amd,
}
impl fmt::Display for Cpu {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}
