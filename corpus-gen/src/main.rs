use clap::Parser;
use fuzzer_data::decoder::InstructionDecoder;
use fuzzer_data::instruction_corpus::{CorpusInstruction, InstructionCorpus};
use iced_x86::FlowControl;
use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Parser)]
#[command(author, version, about)]
struct Args {
    /// Path to library folder
    folder: String,
    /// Output file
    output: PathBuf,
}

fn main() {
    let args = Args::parse();

    let folder_path = Path::new(&args.folder);
    if !folder_path.exists() || !folder_path.is_dir() {
        eprintln!("Error: The specified folder does not exist or is not a directory.");
        std::process::exit(1);
    }

    if args.output.exists() {
        let file = std::fs::File::open(&args.output);
        if let Ok(file) = file {
            let corpus_check: Result<InstructionCorpus, _> = serde_json::from_reader(file);
            if corpus_check.is_err() {
                eprintln!("Error: Output file exists but is not a valid Corpus format.");
                std::process::exit(1);
            }
        } else {
            eprintln!("Error: Failed to open the output file.");
            std::process::exit(1);
        }
    }

    let library_files: Vec<_> = std::fs::read_dir(folder_path)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .path()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains(".so")
                || if let Some(ext) = entry.path().extension() {
                    ext == "a"
                } else {
                    false
                }
        })
        .map(|entry| entry.path())
        .collect();

    println!(
        "Library files:\n - {}",
        library_files
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join("\n - ")
    );

    let mut result = BTreeSet::new();

    let mut decoder = InstructionDecoder::new();

    for path in library_files {
        println!("Processing file: {:?}", path);
        result.extend(process_binary(&path, &mut decoder));
    }

    let corpus = InstructionCorpus {
        instructions: result,
    };

    println!("Found {} unique instructions", corpus.instructions.len());
    println!(
        " - Valid instructions: {}",
        corpus.instructions.iter().filter(|i| i.valid).count()
    );
    println!(
        " - Invalid instructions: {}",
        corpus.instructions.iter().filter(|i| !i.valid).count()
    );

    serde_json::to_writer(std::fs::File::create(&args.output).unwrap(), &corpus).unwrap();
}

fn process_binary<A: AsRef<Path>>(
    path: A,
    decoder: &mut InstructionDecoder,
) -> BTreeSet<CorpusInstruction> {
    let temp_dir = tempfile::tempdir().expect("Failed to create temporary directory");
    let temp_dir_path = temp_dir.path();

    let mut result = BTreeSet::new();

    if path.as_ref().extension().unwrap() == "a" {
        let mut ar_cmd = Command::new("ar");
        ar_cmd
            .arg("x")
            .arg(path.as_ref().canonicalize().unwrap())
            .current_dir(&temp_dir_path)
            .stderr(Stdio::piped())
            .stdout(Stdio::piped());

        let ar_output = ar_cmd.output().expect("Failed to execute ar command");
        if !ar_output.status.success() {
            eprintln!(
                "Error: ar command failed with status {}: {}",
                ar_output.status,
                String::from_utf8_lossy(&ar_output.stderr)
            );
            return BTreeSet::new();
        }

        let mut counter_include = 0;
        let mut counter_exclude = 0;

        for entry in std::fs::read_dir(&temp_dir_path).expect("Failed to read temporary directory")
        {
            let entry_path = entry.expect("Failed to read directory entry").path();
            if entry_path.is_file() {
                if entry_path.extension().unwrap() != "o" {
                    continue;
                }

                let target_file = entry_path.with_extension("text");

                // println!("   {entry_path:?} -> {target_file:?}");

                let mut cmd = Command::new("objcopy");
                cmd.arg("-O")
                    .arg("binary")
                    .arg("--only-section=.text")
                    .arg("--set-section-flags")
                    .arg(".text=alloc")
                    .arg(&entry_path)
                    .arg(&target_file)
                    .stderr(Stdio::piped())
                    .stdout(Stdio::piped());

                let output = cmd.output().expect("Failed to execute objcopy");
                if !output.status.success() {
                    eprintln!(
                        "Error: objcopy failed with status {}: {}",
                        output.status,
                        String::from_utf8_lossy(&output.stderr)
                    );
                }

                if let Ok(mut file) = std::fs::File::open(&target_file) {
                    let mut buffer = Vec::new();
                    file.read_to_end(&mut buffer)
                        .expect("Failed to read target file");

                    let decoded = decoder.decode(&buffer);
                    for i in 0..decoded.len() {
                        let instruction = decoded.get(i).unwrap();

                        if matches!(
                            instruction.instruction.flow_control(),
                            FlowControl::Call
                                | FlowControl::IndirectCall
                                | FlowControl::Return
                                | FlowControl::IndirectBranch
                                | FlowControl::ConditionalBranch
                                | FlowControl::UnconditionalBranch
                        ) {
                            counter_exclude += 1;
                            continue;
                        }

                        result.insert(CorpusInstruction {
                            bytes: instruction.bytes.to_vec(),
                            valid: !instruction.instruction.is_invalid(),
                        });
                    }
                }
            }
        }
    } else {
        // so file
        let target_file = temp_dir_path
            .join(
                path.as_ref()
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
            )
            .with_extension("text");

        // println!("   {entry_path:?} -> {target_file:?}");

        let mut cmd = Command::new("objcopy");
        cmd.arg("-O")
            .arg("binary")
            .arg("--only-section=.text")
            .arg("--set-section-flags")
            .arg(".text=alloc")
            .arg(path.as_ref())
            .arg(&target_file)
            .stderr(Stdio::piped())
            .stdout(Stdio::piped());

        let output = cmd.output().expect("Failed to execute objcopy");
        if !output.status.success() {
            eprintln!(
                "Error: objcopy failed with status {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            );
        }

        if let Ok(mut file) = std::fs::File::open(&target_file) {
            let mut buffer = Vec::new();
            file.read_to_end(&mut buffer)
                .expect("Failed to read target file");

            let decoded = decoder.decode(&buffer);
            for i in 0..decoded.len() {
                let instruction = decoded.get(i).unwrap();

                if matches!(
                    instruction.instruction.flow_control(),
                    FlowControl::Call
                        | FlowControl::IndirectCall
                        | FlowControl::Return
                        | FlowControl::IndirectBranch
                        | FlowControl::ConditionalBranch
                        | FlowControl::UnconditionalBranch
                ) {
                    continue;
                }

                result.insert(CorpusInstruction {
                    bytes: instruction.bytes.to_vec(),
                    valid: !instruction.instruction.is_invalid(),
                });
            }
        }
    }

    result
}
