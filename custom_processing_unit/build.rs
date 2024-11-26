use std::path::PathBuf;
use std::process::Command;
use std::{env, fs};
use ucode_compiler::uasm::AUTOGEN;

fn main() {
    println!("cargo::rerun-if-changed=build.rs");

    if let Err(err) =
        ucode_compiler::uasm::compile_source_and_create_module("patches", "src/patches", false)
    {
        panic!("Failed to compile: {:?}", err);
    }
    generate_labels();
}

fn generate_labels() {
    let current_directory =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("Cargo must include Manifest dir"));
    let cpu_ucode_labels = current_directory
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("CustomProcessingUnit")
        .join("uasm-lib")
        .join("labels.csv");

    println!(
        "cargo:rerun-if-changed={}",
        cpu_ucode_labels.to_string_lossy()
    );

    let content = fs::read_to_string(cpu_ucode_labels).expect("Unable to read file with labels");

    let mut labels = String::new();

    labels.push_str(AUTOGEN);
    labels.push_str("\nuse data_types::addresses::UCInstructionAddress;\n");

    for line in content.lines() {
        let split = line.split(',').collect::<Vec<&str>>();
        if split.len() != 2 {
            continue;
        }
        let (label, address) = (split[0].replace("#", ""), split[1]);

        if !label.ends_with("_xlat") {
            continue;
        }

        labels.push_str(&format!(
            "/// Ucode address of the instruction {}\n",
            label.split("_").next().unwrap_or("UNKNOWN")
        ));

        labels.push_str(&format!(
            "pub const {}: UCInstructionAddress = UCInstructionAddress::from_const(0x{});\n",
            label.to_uppercase(),
            address
        ));
    }

    let target_file = current_directory.join("src").join("labels.rs");
    if target_file.exists() && !fs::read_to_string(&target_file).unwrap().contains(AUTOGEN) {
        panic!("labels.rs already exists and does not contain the AUTOGEN_NOTICE");
    }
    fs::write(&target_file, labels).expect("Unable to write file labels.rs");

    let _ = Command::new("rustfmt")
        .arg(target_file)
        .spawn()
        .expect("rustfmt not found")
        .wait()
        .expect("rustfmt failed");
}
