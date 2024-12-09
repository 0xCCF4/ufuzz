use std::fs;
use std::path::{Path, PathBuf};
use ucode_compiler::uasm::{run_rustfmt, CompilerOptions, AUTOGEN};

fn main() {
    println!("cargo::rerun-if-changed=build.rs");

    if let Err(err) = ucode_compiler::uasm::compile_source_and_create_module(
        "patches",
        "src/patches",
        CompilerOptions {
            allow_unused: false,
            cpuid: None,
            avoid_unknown_256: true,
        },
    ) {
        panic!("Failed to compile: {:?}", err);
    }

    create_cpu_files();
}

fn create_cpu_files() {
    let target_folder = PathBuf::from("src/dump");
    let model_folder = PathBuf::from("models");
    let module_file = PathBuf::from("src/dump.rs");

    if !target_folder.exists() {
        fs::create_dir(&target_folder).expect("Failed to create dump output folder");
    }

    let mut module_file_contents = String::new();
    module_file_contents.push_str(AUTOGEN);
    module_file_contents.push_str("\npub use crate::RomDump;\n");

    for cpu_arch in model_folder
        .read_dir()
        .expect("models directory does not exist")
    {
        let cpu_arch = cpu_arch.expect("Failed to read directory entry").path();

        if cpu_arch.is_dir() {
            let cpu_model = cpu_arch
                .file_name()
                .expect("Failed to get file name")
                .to_str()
                .expect("Failed to convert to string");
            let cpu_model_name = format!("cpu_{}", cpu_model);

            let arrays = read_arrays(&cpu_arch);
            let array_dump = generate_ms_array_file(&arrays);
            let labels = generate_labels(cpu_arch.join("labels.csv"));

            let file_content = format!("{}\n{}", array_dump, labels);
            let file_path = target_folder.join(format!("{}.rs", cpu_model_name));

            fs::write(&file_path, file_content).expect("Failed to write file");
            run_rustfmt(&file_path);

            module_file_contents.push_str(&format!(
                "#[allow(non_snake_case)]\npub mod {};\n",
                cpu_model_name
            ));

            module_file_contents.push_str(format!("#[allow(non_snake_case, non_upper_case_globals)] pub const ROM_{cpu_model_name}: RomDump<'static, 'static> = RomDump::new(&{cpu_model_name}::ROM_INSTRUCTION, &{cpu_model_name}::ROM_SEQUENCE);\n").as_str());
        }
    }

    fs::write(&module_file, module_file_contents).expect("Failed to write module file");
    run_rustfmt(&module_file);
}

fn generate_labels<P: AsRef<Path>>(labels: P) -> String {
    println!(
        "cargo:rerun-if-changed={}",
        labels.as_ref().to_string_lossy()
    );

    let content = fs::read_to_string(labels.as_ref()).expect("Unable to read file with labels");

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

    labels
}

fn read_arrays<A: AsRef<Path>>(parent_folder: A) -> Vec<Vec<u64>> {
    let mut result = Vec::new();

    for i in 0..5 {
        let file = parent_folder.as_ref().join(format!("ms_array{}.txt", i));
        println!("cargo:rerun-if-changed={}", file.to_string_lossy());
        let mut array = parse_rom_file(file);

        if i == 0 {
            // instructions ROM
            array.truncate(0x7c00)
        } else if i == 1 {
            // sequence words ROM
            array = array.into_iter().enumerate().filter(|(i, _)| i % 4 == 0).map(|(_, v)| v).collect::<Vec<u64>>();
            array.truncate(0x7c00 / 4)
        }

        result.push(array);
    }

    result
}

fn generate_ms_array_file(arrays: &Vec<Vec<u64>>) -> String {
    let descriptions = [
        "/// The microcode firmware dump: ROM Instruction memory\n",
        "/// The microcode firmware dump: ROM Sequence memory\n",
        "/// The microcode firmware dump: RAM Sequence memory\n",
        "/// The microcode firmware dump: RAM Hook memory\n",
        "/// The microcode firmware dump: RAM Instruction memory\n",
    ];

    let mut result = "".to_string();

    for (i, (comment, content)) in descriptions.iter().zip(arrays).enumerate() {
        #[allow(clippy::manual_range_patterns)]
        let data_type = match i {
            1 | 2 | 3 => "u32",
            0 | 4 => "u64",
            _ => "u64",
        };

        result.push_str(comment);
        result.push_str(
            format!(
                "pub const MS_ARRAY_{i}: [{data_type}; {}] = [\n",
                content.len()
            )
            .as_str(),
        );
        for value in content {
            result.push_str(format!("    0x{:016x},\n", value).as_str());
        }
        result.push_str("];\n\n");
    }

    result.push_str("pub use MS_ARRAY_0 as ROM_INSTRUCTION;\n");
    result.push_str("pub use MS_ARRAY_1 as ROM_SEQUENCE;\n");
    result.push_str("pub use MS_ARRAY_2 as RAM_SEQUENCE;\n");
    result.push_str("pub use MS_ARRAY_3 as RAM_HOOK;\n");
    result.push_str("pub use MS_ARRAY_4 as RAM_INSTRUCTION;\n");

    result.push_str("\n\n");

    result
}

fn parse_rom_file<A: AsRef<Path>>(file: A) -> Vec<u64> {
    println!(
        "cargo:rerun-if-changed={}",
        file.as_ref().to_str().expect("path not utf8")
    );
    let file = std::fs::read_to_string(file).expect("read failed");
    file.lines()
        .filter_map(|line| {
            let line = line.trim();
            if let Some((_addr, content)) = line.split_once(":") {
                let content = content.trim();
                let values = content.split(" ").map(|v| v.trim()).collect::<Vec<&str>>();
                assert_eq!(values.len(), 4);

                let mut result = [0u64; 4];

                for (value, r) in values.iter().zip(result.iter_mut()) {
                    let parsed_hex = u64::from_str_radix(value, 16).expect("parse failed");
                    *r = parsed_hex;
                }

                Some(result)
            } else {
                None
            }
        })
        .flat_map(|v| v.into_iter())
        .collect::<Vec<u64>>()
}
