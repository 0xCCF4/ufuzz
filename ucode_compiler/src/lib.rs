#![cfg_attr(not(feature = "build"), no_std)]

#[cfg(feature = "build")]
use error_chain::error_chain;
#[cfg(feature = "build")]
use std::path::PathBuf;
#[cfg(feature = "build")]
use std::process::Command;
#[cfg(feature = "build")]
use std::{env, fs};

pub type UcodePatchEntry = [usize; 4];
pub type UcodePatchBlob = [UcodePatchEntry];

pub struct Patch<'a> {
    pub addr: usize,
    pub hook_address: Option<usize>,
    pub hook_entry: Option<usize>,
    pub ucode_patch: &'a UcodePatchBlob,
}

#[cfg(feature = "build")]
error_chain! {
    errors {
        CompilerNotFound {
            description("Compiler not found")
            display("Compiler not found. Path does not exist")
        }
        CompilerInvocationError {
            description("Compiler invocation error")
            display("Compiler invocation error")
        }
        CompilationFailed(exit_code: i32) {
            description("Compiler invocation error")
            display("Compiler invocation error. Exit code: {}", exit_code)
        }
        SourceFileDoesNotExist(path: PathBuf) {
            description("Source file does not exist")
            display("Source file {:?} does not exist", path)
        }
    }

    skip_msg_variant
}

#[cfg(feature = "build")]
pub struct UcodeCompiler {
    compiler_path: PathBuf,
}

#[cfg(feature = "build")]
impl UcodeCompiler {
    pub fn new(path: PathBuf) -> Result<UcodeCompiler> {
        if !path.exists()
            || path
                .extension()
                .map(|v| v.to_string_lossy().to_string())
                .unwrap_or("".to_string())
                != "py"
        {
            return Err(ErrorKind::CompilerNotFound.into());
        }

        Ok(UcodeCompiler {
            compiler_path: path,
        })
    }

    pub fn compile(
        &self,
        cpuid: Option<String>,
        input: &PathBuf,
        output: &PathBuf,
        avoid_unknown_256: bool,
    ) -> Result<()> {
        if !input.exists() {
            return Err(ErrorKind::SourceFileDoesNotExist(input.to_owned()).into());
        }

        let mut command = Command::new("python3");

        match self.compiler_path.to_str() {
            None => return Err(ErrorKind::CompilerNotFound.into()),
            Some(path) => command.arg(path),
        };

        if let Some(cpuid) = cpuid {
            command.arg("--cpuid").arg(cpuid);
        }

        command.arg("-i").arg(input).arg("-o").arg(output);

        if avoid_unknown_256 {
            command.arg("--avoid_unk_256");
        }

        let status = command
            .status()
            .map_err(|_| ErrorKind::CompilerInvocationError)?;

        if status.success() {
            Ok(())
        } else {
            match status.code() {
                Some(code) => Err(ErrorKind::CompilationFailed(code).into()),
                None => Err(ErrorKind::CompilerInvocationError.into()),
            }
        }
    }
}

#[cfg(feature = "build")]
pub fn build_script(source_folder: &PathBuf, target_folder: &PathBuf) -> Vec<String> {
    println!("cargo::rerun-if-changed=build.rs");

    let current_directory =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("Cargo must include Manifest dir"));

    if !current_directory.exists() {
        panic!("CARGO_MANIFEST_DIR directory does not exist");
    }
    if !source_folder.exists() {
        panic!("Source folder does not exist");
    }
    if !target_folder.exists() {
        panic!("Target folder does not exist");
    }

    println!(
        "cargo::rerun-if-changed={}",
        source_folder.to_string_lossy()
    );

    let ucode_compiler = current_directory
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("CustomProcessingUnit")
        .join("uasm-lib")
        .join("uasm.py");

    let compiler = UcodeCompiler::new(ucode_compiler).expect("Ucode compiler not found");

    let mut patch_names = Vec::new();

    for patch in source_folder
        .read_dir()
        .expect("Patches directory not readable")
    {
        let patch = patch.expect("Patch not found").path();

        if patch
            .extension()
            .map(|v| v.to_str().unwrap_or(&""))
            .unwrap_or(&"")
            == "u"
        {
            println!("cargo::rerun-if-changed={}", patch.to_string_lossy());
            println!("Compiling: {:?}", patch);

            let target_file =
                target_folder.join(patch.file_name().expect("Patch file name not found"));
            patch_names.push(
                target_file
                    .with_extension("")
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
            );

            compiler
                .compile(None, &patch, &target_file.with_extension("h"), true)
                .expect("Compilation failed");
            transform_patch(
                &target_file.with_extension("h"),
                &target_file.with_extension("rs"),
            );
            fs::remove_file(&target_file.with_extension("h")).expect("Patch file not removed");
        }
    }

    patch_names
}

#[cfg(feature = "build")]
fn transform_patch(patch: &PathBuf, target: &PathBuf) {
    // Transform the patch file from C-header to rust

    let content = std::fs::read_to_string(patch).expect("Patch file not readable");

    let mut addr = None;
    let mut hook_address = None;
    let mut hook_entry = None;

    let regex_variables =
        regex::Regex::new("unsigned long (addr|hook_address|hook_entry) = (0[xX][0-9a-fA-F]+);")
            .expect("regex compile error");

    for capture in regex_variables.captures_iter(&content) {
        let content = capture
            .get(2)
            .expect("Capture not found")
            .as_str()
            .to_string();
        if let Some(variable) = capture.get(1) {
            match variable.as_str() {
                "addr" => addr = Some(content),
                "hook_address" => hook_address = Some(content),
                "hook_entry" => hook_entry = Some(content),
                _ => unreachable!("Unknown variable"),
            }
        }
    }

    let regex_array = regex::Regex::new("unsigned long ucode_patch\\[]\\[4] = \\{\n(([^}]*[^;])*)")
        .expect("Regex compile error");

    let patch = regex_array
        .captures_iter(&content)
        .into_iter()
        .next()
        .expect("No patch block")
        .get(1)
        .expect("No match")
        .as_str();
    let patch = patch.replace("    {", "    [").replace("},\n", "],\n");
    let patch = &patch[0..patch.len() - 4];
    let length = patch.lines().count() / 2;

    let content = format!(
        "use ucode_compiler::{{UcodePatchEntry, Patch}};

        pub const PATCH: Patch<'static> = Patch {{
            addr: {},
            ucode_patch: &UCODE_PATCH,
            hook_address: {},
            hook_entry: {},
        }};

        const UCODE_PATCH: [UcodePatchEntry; {length}] = [\n{}
        ];
        ",
        addr.expect("No address found")
            .to_string()
            .replace("\"", ""),
        format!("{:?}", hook_address).replace("\"", ""),
        format!("{:?}", hook_entry).replace("\"", ""),
        patch
    );

    std::fs::write(target, content).expect("Patch file not writable");
}
