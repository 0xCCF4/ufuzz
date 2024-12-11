use error_chain::error_chain;
use regex::{Captures, Replacer};
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::{env, fs};
use std::ffi::OsStr;

error_chain! {
    errors {
        CompilerNotFound(path: PathBuf) {
            description("Compiler not found")
            display("Compiler not found {:?}. Path does not exist. Try setting the env variable UASM.", path)
        }
        CompilerInvocationError {
            description("Compiler invocation error")
            display("Compiler invocation error")
        }
        CompilationFailed(exit_code: i32, stdout: String) {
            description("Compiler invocation error")
            display("Compiler invocation error. Exit code: {}.\n{}", exit_code, stdout)
        }
        SourceFileDoesNotExist(path: PathBuf) {
            description("Source file does not exist")
            display("Source file {:?} does not exist", path)
        }
        TargetFolderDoesNotExist(path: PathBuf) {
            description("Target folder does not exist")
            display("Target folder {:?} does not exist", path)
        }
        FailedToWrite(path: PathBuf, description: String, error: std::io::Error) {
            description("Failed to write file")
            display("Failed to write file {:?} at step {}: {}", path, description, error)
        }
        FailedToRead(path: PathBuf, description: String, error: std::io::Error) {
            description("Failed to read file")
            display("Failed to read file {:?} at step {}: {}", path, description, error)
        }
        LossyFilenameConversion(path: PathBuf) {
            description("Failed to convert filename to string")
            display("Failed to convert filename {:?} to string, since it contains non UTF-8 characters", path)
        }
        ParentDirectoryReadError(path: PathBuf, description: String) {
            description("Failed to read parent directory")
            display("Failed to read parent directory of {:?} at {}", path, description)
        }
        FileDeletionError(path: PathBuf, description: String, error: std::io::Error) {
            description("Failed to delete file")
            display("Failed to delete file {:?} at {}: {}", path, description, error)
        }
        PreprocessorError(path: PathBuf, description: String) {
            description("Preprocessor error")
            display("Preprocessor error at {:?}: {}", path, description)
        }
        FileExistsButNoAutogen(path: PathBuf) {
            description("File exists but does not contain AUTOGEN_NOTICE")
            display("File {:?} exists but does not contain AUTOGEN_NOTICE", path)
        }
    }

    skip_msg_variant
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilerOptions {
    pub cpuid: Option<String>,
    pub avoid_unknown_256: bool,
    pub allow_unused: bool,
}

impl AsRef<CompilerOptions> for CompilerOptions {
    fn as_ref(&self) -> &CompilerOptions {
        self
    }
}

pub fn run_rustfmt<P: AsRef<Path>>(path: P) {
    let _ = Command::new("rustfmt")
        .arg(path.as_ref())
        .spawn()
        .map(|mut c| c.wait());
}

pub struct UcodeCompiler {
    compiler_path: PathBuf,
}

impl UcodeCompiler {
    pub fn new(path: PathBuf) -> Result<UcodeCompiler> {
        if !path.exists()
            || path
                .extension()
                .map(|v| v.to_string_lossy().to_string())
                .unwrap_or("".to_string())
                != "py"
        {
            return Err(ErrorKind::CompilerNotFound(path).into());
        }

        Ok(UcodeCompiler {
            compiler_path: path,
        })
    }

    pub fn compile<C: AsRef<CompilerOptions>>(
        &self,
        input: &PathBuf,
        output: &PathBuf,
        compiler_options: C,
    ) -> Result<()> {
        if !input.exists() {
            return Err(ErrorKind::SourceFileDoesNotExist(input.to_owned()).into());
        }

        let mut command = Command::new("python3");

        match self.compiler_path.to_str() {
            None => {
                return Err(ErrorKind::LossyFilenameConversion(self.compiler_path.clone()).into())
            }
            Some(path) => command.arg(path),
        };

        if let Some(cpuid) = compiler_options.as_ref().clone().cpuid {
            command.arg("--cpuid").arg(cpuid);
        }

        command.arg("-i").arg(input).arg("-o").arg(output);

        if compiler_options.as_ref().avoid_unknown_256 {
            command.arg("--avoid_unk_256");
        }

        let mut cmd = command
            .spawn()
            .map_err(|_| ErrorKind::CompilerInvocationError)?;

        let result = cmd.wait().map_err(|_| ErrorKind::CompilerInvocationError)?;

        let mut error_text = String::new();

        if let Some(mut stdout) = cmd.stdout {
            let mut output = String::new();
            stdout
                .read_to_string(&mut output)
                .map_err(|_| ErrorKind::CompilerInvocationError)?;
            println!("{}", output);
            error_text.push_str(&output);
        }
        if let Some(mut stderr) = cmd.stderr {
            let mut output = String::new();
            stderr
                .read_to_string(&mut output)
                .map_err(|_| ErrorKind::CompilerInvocationError)?;
            eprintln!("{}", output);
            error_text.push_str(&output);
        }

        if result.success() {
            Ok(())
        } else {
            match result.code() {
                Some(code) => Err(ErrorKind::CompilationFailed(code, error_text).into()),
                None => Err(ErrorKind::CompilerInvocationError.into()),
            }
        }
    }
}

pub const AUTOGEN: &str = "// AUTOGEN_NOTICE: this file is automatically generated. Do not change stuff. This file will be overriden without further notice.";

pub const AUTOGEN_PREFIX: &str = "// AUTOGEN_NOTICE: ";

/// Compiles all source files and creates a patches.rs module file with all source files registered to.
pub fn compile_source_and_create_module<
    P: AsRef<Path>,
    Q: AsRef<Path>,
    C: AsRef<CompilerOptions>,
>(
    patch_source_folder: P,
    target_rust_folder: Q,
    compiler_options: C,
) -> Result<()> {
    if !target_rust_folder.as_ref().exists() {
        fs::create_dir(&target_rust_folder).or_else(|err| {
            Err(ErrorKind::FailedToWrite(
                target_rust_folder.as_ref().to_owned(),
                "target folder creation".to_string(),
                err,
            ))
        })?;
    } else {
        for file in target_rust_folder.as_ref().read_dir().or_else(|e| {
            Err(ErrorKind::FailedToRead(
                target_rust_folder.as_ref().to_owned(),
                "compile: delete".to_string(),
                e,
            ))
        })? {
            let file = file
                .or_else(|err| {
                    Err(ErrorKind::FailedToRead(
                        target_rust_folder.as_ref().to_owned(),
                        "compile: delete content".to_string(),
                        err,
                    ))
                })?
                .path();
            if file
                .extension()
                .map(|v| v.to_string_lossy().to_string())
                .unwrap_or("".to_string())
                .as_str()
                == "rs"
            {
                if std::fs::read_to_string(&file)
                    .or_else(|e| {
                        Err(ErrorKind::FailedToRead(
                            file.to_owned(),
                            "target file".to_string(),
                            e,
                        ))
                    })?
                    .starts_with(AUTOGEN_PREFIX)
                {
                    fs::remove_file(&file).or_else(|e| {
                        Err(ErrorKind::FileDeletionError(
                            file.clone(),
                            "compile folder".to_string(),
                            e,
                        ))
                    })?;
                } else {
                    return Err(ErrorKind::FileExistsButNoAutogen(file.clone()).into());
                }
            }
        }
    }

    let module_path = target_rust_folder.as_ref().with_extension("rs");
    if module_path.exists() {
        // try to check if AUTOGEN_NOTICE
        let content: Result<String> = match std::fs::read_to_string(&module_path) {
            Ok(v) => Ok(v),
            Err(e) => {
                Err(
                    ErrorKind::FailedToRead(module_path.to_owned(), "module source".to_string(), e)
                        .into(),
                )
            }
        };
        let content = content?;
        if !content.starts_with(AUTOGEN_PREFIX) {
            return Err(ErrorKind::FileExistsButNoAutogen(module_path.to_owned()).into());
        }
    }

    let names =
        build_script_compile_folder(patch_source_folder, &target_rust_folder, &compiler_options)?;

    let allow_import = if compiler_options.as_ref().allow_unused {
        "#[allow(unused_imports)]\n"
    } else {
        ""
    };

    let content = names.iter().fold(String::new(), |acc, name| {
        acc + &format!("pub mod {name};\n/*\n{allow_import}pub use {name}::PATCH as {name};\n*/\n")
    });
    if let Err(err) = std::fs::write(&module_path, format!("{AUTOGEN}\n\n{content}").as_str()) {
        return Err(ErrorKind::FailedToWrite(
            module_path.to_owned(),
            "module source".to_string(),
            err,
        )
        .into());
    }

    run_rustfmt(module_path);

    Ok(())
}

// Compiles all source files in the source dir -> target dir
pub fn build_script_compile_folder<P: AsRef<Path>, Q: AsRef<Path>, C: AsRef<CompilerOptions>>(
    patch_source_folder: P,
    target_rust_folder: Q,
    compiler_options: C,
) -> Result<Vec<String>> {
    let current_directory =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("Cargo must include Manifest dir"));

    if !current_directory.exists() {
        panic!("CARGO_MANIFEST_DIR directory does not exist");
    }
    if !patch_source_folder.as_ref().exists() {
        return Err(
            ErrorKind::SourceFileDoesNotExist(patch_source_folder.as_ref().to_owned()).into(),
        );
    }
    if !target_rust_folder.as_ref().exists() {
        return Err(
            ErrorKind::TargetFolderDoesNotExist(target_rust_folder.as_ref().to_owned()).into(),
        );
    }

    if !patch_source_folder.is_temporary() {
        println!(
            "cargo::rerun-if-changed={}",
            patch_source_folder.as_ref().to_string_lossy()
        );
        println!(
            "cargo::rerun-if-changed={}/*",
            patch_source_folder.as_ref().to_string_lossy()
        );
    }

    let ucode_compiler = if let Ok(path) = env::var("UASM") {
        PathBuf::from(path).join("uasm.py")
    } else {
        current_directory
            .parent()
            .ok_or_else(|| {
                ErrorKind::ParentDirectoryReadError(
                    current_directory.clone(),
                    "searching uasm.py 1".to_string(),
                )
            })?
            .parent()
            .ok_or_else(|| {
                ErrorKind::ParentDirectoryReadError(
                    current_directory.clone(),
                    "searching uasm.py 2".to_string(),
                )
            })?
            .join("CustomProcessingUnit")
            .join("uasm-lib")
            .join("uasm.py")
    };

    println!(
        "cargo::rerun-if-changed={}",
        ucode_compiler.to_string_lossy()
    );

    let compiler = UcodeCompiler::new(ucode_compiler)?;

    let mut patch_names = Vec::new();

    for patch in patch_source_folder.as_ref().read_dir().or_else(|e| {
        Err(ErrorKind::FailedToRead(
            patch_source_folder.as_ref().to_owned(),
            "compile folder".to_string(),
            e,
        ))
    })? {
        let patch = patch
            .or_else(|err| {
                Err(ErrorKind::FailedToRead(
                    patch_source_folder.as_ref().to_owned(),
                    "compile folder content".to_string(),
                    err,
                ))
            })?
            .path();

        if patch
            .extension()
            .map(|v| v.to_str().unwrap_or(""))
            .unwrap_or("")
            == "u"
        {
            if !patch.is_temporary() {
                println!("cargo::rerun-if-changed={}", patch.to_string_lossy());
            }
            println!("Compiling: {:?}", patch);

            let target_file = target_rust_folder
                .as_ref()
                .join(patch.file_name().expect("Patch file name not found"));
            patch_names.push(
                target_file
                    .with_extension("")
                    .file_name()
                    .unwrap()
                    .to_str()
                    .ok_or_else(|| ErrorKind::LossyFilenameConversion(target_file.clone()))?
                    .to_string(),
            );

            compiler.compile(&patch, &target_file.with_extension("h"), &compiler_options)?;
            transform_h_patch_to_rs_patch(
                target_file.with_extension("h"),
                target_file.with_extension("rs"),
                compiler_options.as_ref().allow_unused,
            )?;
            fs::remove_file(target_file.with_extension("h")).or_else(|e| {
                Err::<(), Error>(
                    ErrorKind::FileDeletionError(
                        target_file.with_extension("h"),
                        "compile folder".to_string(),
                        e,
                    )
                    .into(),
                )
            })?;
        }
    }

    Ok(patch_names)
}

pub fn transform_h_patch_to_rs_patch<P: AsRef<Path>, Q: AsRef<Path>>(
    patch: P,
    target: Q,
    allow_unused: bool,
) -> Result<()> {
    // Transform the patch file from C-header to rust

    let content = std::fs::read_to_string(&patch).or_else(|e| {
        Err::<String, Error>(
            ErrorKind::FailedToRead(patch.as_ref().to_owned(), "compile file".to_string(), e)
                .into(),
        )
    })?;

    let mut addr = None;
    let mut hook_address = None;
    let mut hook_entry = None;

    let allow_dead = if allow_unused {
        "#[allow(dead_code)]"
    } else {
        ""
    };

    let mut labels = Vec::default();

    let regex_labels = regex::Regex::new("unsigned long LABEL_([^ ]+) = (0[xX][0-9a-fA-F]+);")
        .expect("regex compile error");
    for capture in regex_labels.captures_iter(&content) {
        let name = capture.get(1).expect("Capture not found").as_str();
        let address = capture.get(2).expect("Capture not found").as_str();

        let public = "pub";

        labels.push((name.to_uppercase(), address, public));
    }
    let labels_count = labels.len();
    let labels_const = labels.iter().map(|(label, value, public)| format!("#[allow(dead_code)]\n{public} const LABEL_{label}: UCInstructionAddress = UCInstructionAddress::from_const({value});")).collect::<Vec<String>>().join("\n");
    let labels_array = labels
        .iter()
        .map(|(label, _value, _public)| format!("({label:?}, LABEL_{label}),"))
        .collect::<Vec<String>>()
        .join("\n");

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

    let hook_address = hook_address
        .map(|v| format!("Some(UCInstructionAddress::from_const({}))", v))
        .unwrap_or("None".to_string());
    let hook_entry = hook_entry
        .map(|v| format!("Some(MSRAMHookIndex::from_const({}))", v))
        .unwrap_or("None".to_string());

    let regex_array = regex::Regex::new("unsigned long ucode_patch\\[]\\[4] = \\{\n(([^}]*[^;])*)")
        .expect("Regex compile error");

    let patch = regex_array
        .captures_iter(&content)
        .next()
        .expect("No patch block")
        .get(1)
        .expect("No match")
        .as_str();
    let patch = patch.replace("    {", "    [").replace("},\n", "],\n");
    let patch = &patch[0..patch.len() - 4];
    let length = patch.lines().count() / 2;

    let mut space_requirement_analysis = String::new();
    space_requirement_analysis.push_str("// Space requirement analysis:\n");

    let mut sorted_labels_by_size = labels
        .iter()
        .filter_map(|(name, address, _)| {
            labels
                .iter()
                .find(|(other_name, _, _)| {
                    format!("{name}_end").to_lowercase() == other_name.to_lowercase()
                })
                .map(|other| (name, address, other.1.to_string()))
        })
        .map(|(name, address, other_address)| {
            (
                name,
                u64::from_str_radix(&address[2..], 16).expect("Address parse error"),
                u64::from_str_radix(&other_address[2..], 16).expect("Address parse error") - 1,
            )
        })
        .map(|(name, address, other_address)| {
            (
                name,
                address,
                other_address,
                if other_address > address {
                    other_address - address
                } else {
                    0
                },
            )
        })
        .collect::<Vec<(&String, u64, u64, u64)>>();

    sorted_labels_by_size.sort_by_key(|(_, _, _, size)| *size);

    for (name, address, other_address, size) in sorted_labels_by_size.iter().rev() {
        println!("{name} {address:04x} -> {other_address:04x} {size:3}");
        assert!(
            other_address >= address,
            "Address with XXX_end postfix must be used later than label XXX"
        );
        let triads = size / 4 + if size % 4 > 0 { 1 } else { 0 };
        space_requirement_analysis.push_str(
            format!("// {size:3}|{triads:3} [{address:04x} -> {other_address:04x}] {name}\n")
                .as_str(),
        )
    }
    if !sorted_labels_by_size.is_empty() {
        space_requirement_analysis.push_str("// ---------------------- \n");
    }
    space_requirement_analysis
        .push_str(format!("// Total: {:3}| {:3} of 128 triads\n", length * 4, length).as_str());

    assert!(length <= 128, "Patch code is too large to fit in MSRAM");

    let content = format!(
        "{AUTOGEN}

        use data_types::{{UcodePatchEntry, Patch, LabelMapping}};
        #[allow(unused_imports)]
        use data_types::addresses::{{UCInstructionAddress, MSRAMHookIndex}};

        {space_requirement_analysis}

        {labels_const}

        const LABELS: [LabelMapping<'static>; {labels_count}] = [{labels_array}];

        {allow_dead}
        pub const PATCH: Patch<'static, 'static, 'static> = Patch {{
            addr: UCInstructionAddress::from_const({}),
            ucode_patch: &UCODE_PATCH_CONTENT,
            hook_address: {},
            hook_index: {},
            labels: &LABELS,
        }};

        {allow_dead}
        pub const UCODE_PATCH_CONTENT: [UcodePatchEntry; {length}] = [\n{}
        ];
        ",
        addr.expect("No address found")
            .to_string()
            .replace("\"", ""),
        format!("{:?}", hook_address).replace("\"", ""),
        format!("{:?}", hook_entry).replace("\"", ""),
        patch
    );

    std::fs::write(&target, content).or_else(|e| {
        Err::<(), Error>(
            ErrorKind::FailedToWrite(target.as_ref().to_owned(), "compile file".to_string(), e)
                .into(),
        )
    })?;

    run_rustfmt(target);

    Ok(())
}

const ERROR_MARKER: &str = "PREPROCESSOR_ERROR_MARKER: ";

struct IncludeReplacer {
    cwd: PathBuf,
}
impl IncludeReplacer {
    pub fn new<P: AsRef<Path>>(cwd: P) -> IncludeReplacer {
        IncludeReplacer {
            cwd: cwd.as_ref().to_owned(),
        }
    }
}
impl Replacer for IncludeReplacer {
    fn replace_append(&mut self, caps: &Captures<'_>, dst: &mut String) {
        let name = caps.get(2).expect("include path not given").as_str();
        let path = self.cwd.join(name);
        if !path.is_temporary() {
            println!("cargo:rerun-if-changed={}", path.to_string_lossy());
        }
        let content = std::fs::read_to_string(&path);

        match content {
            Ok(content) => {
                dst.push_str(format!("#------------- INCLUDE {name}\n").as_str());
                dst.push_str(content.trim());
                dst.push_str(format!("\n#------------- END INCLUDE {name}\n").as_str());
            }
            Err(err) => {
                dst.push_str(format!("#------------- FAILED INCLUDE {name}\n").as_str());
                dst.push_str(ERROR_MARKER);
                dst.push_str(
                    format!(
                        "Error in include, the file {:?} could not be read: {:?}\n",
                        path, err
                    )
                    .as_str(),
                );
                dst.push_str(format!("#------------- END FAILED INCLUDE {name}\n").as_str());
            }
        }
    }
}

struct FuncIncludeReplacer {
    cwd: PathBuf,
}
impl FuncIncludeReplacer {
    pub fn new<P: AsRef<Path>>(cwd: P) -> FuncIncludeReplacer {
        FuncIncludeReplacer {
            cwd: cwd.as_ref().to_owned(),
        }
    }
}
impl Replacer for FuncIncludeReplacer {
    fn replace_append(&mut self, caps: &Captures<'_>, dst: &mut String) {
        let name = caps.get(1).expect("func name not given").as_str();
        let args = caps.get(2).expect("func args not given").as_str();

        let args_regex = regex::Regex::new(r" *([^,) ]+) *").expect("regex compile error");
        let args = args_regex
            .captures_iter(args)
            .map(|c| c.get(1).expect("arg not found").as_str())
            .collect::<Vec<&str>>();

        let path = self.cwd.join(name).with_extension("func");
        if !path.is_temporary() {
            println!("cargo:rerun-if-changed={}", path.to_string_lossy());
        }
        let content = std::fs::read_to_string(&path);

        match content {
            Ok(mut content) => {
                let rewrite_regex = regex::Regex::new(r"(?m)^# *(ARG\d+) ?: ?(\S+)[^\n]*\n")
                    .expect("regex compile error");
                for cap in rewrite_regex.captures_iter(content.clone().as_str()) {
                    let arg = cap.get(1).expect("arg not found").as_str();
                    let value = cap.get(2).expect("value not found").as_str();
                    content = content.replace(value, arg);
                }
                content = rewrite_regex.replace_all(&content, "").to_string();

                for (i, arg) in args.iter().enumerate().rev() {
                    content = content.replace(format!("ARG{i}").as_str(), arg);
                }

                dst.push_str(format!("#------------- FUNCTION {name}\n").as_str());
                dst.push_str(content.trim());
                dst.push_str(format!("\n#------------- END FUNCTION {name}\n").as_str());
            }
            Err(err) => {
                dst.push_str(format!("#------------- FAILED FUNCTION {name}\n").as_str());
                dst.push_str(
                    format!(
                        "Error in function, the file {:?} could not be read: {}\n",
                        path, err
                    )
                    .as_str(),
                );
                dst.push_str(format!("#------------- END FAILED FUNCTION {name}\n").as_str());
            }
        }
    }
}

#[derive(Default)]
struct DefineResolveReplacer {
    pub defines: Vec<(String, String)>,
}

impl Replacer for &mut DefineResolveReplacer {
    fn replace_append(&mut self, caps: &Captures<'_>, dst: &mut String) {
        let name = caps.get(2).expect("define name not given").as_str().trim();
        let value = caps.get(3).expect("define value not given").as_str().trim();

        self.defines.push((name.to_string(), value.to_string()));

        dst.push_str(format!("# def {value}\n").as_str());
    }
}

#[derive(Default)]
struct RepeatReplacer {}

impl Replacer for RepeatReplacer {
    fn replace_append(&mut self, caps: &Captures<'_>, dst: &mut String) {
        let number = caps.get(2).expect("repeat number not given").as_str();
        let content = caps.get(3).expect("repeat content not given").as_str();

        let amount = number.parse::<usize>();

        match amount {
            Ok(amount) => {
                dst.push_str("#------------- REPEAT\n");
                for i in 0..amount {
                    dst.push_str(format!("# REP: {i}\n").as_str());
                    dst.push_str(content.trim().replace("\\n", "\n").as_str());
                    dst.push('\n');
                }
                dst.push_str("#------------- END REPEAT\n");
            }
            Err(_) => {
                dst.push_str("#------------- FAILED REPEAT\n");
                dst.push_str(ERROR_MARKER);
                dst.push_str(
                    format!(
                        "Error in repeat, the repeat number could not be parsed: {:?}\n",
                        number
                    )
                    .as_str(),
                );
                dst.push_str("#------------- END FAILED REPEAT\n");
            }
        }
    }
}

/// Execute the preprocessing stage on all files in the source directory and write the processed files to the destination directory.
pub fn preprocess_scripts<A: AsRef<Path>, B: AsRef<Path>, C: AsRef<Path>>(
    src: A,
    dst: B,
    cwd: C,
) -> Result<()> {
    let include_regex =
        regex::Regex::new(r"(?m)^ *include( <?([^>]+)>?)?( *#.*)?$").expect("regex compile error");
    let func_include_regex =
        regex::Regex::new(r"(?m)^ *func *([\S/]+) *\(([^,)]*(, ?[^,)]*)*)\)( *#.*)?$")
            .expect("regex compile error");
    let define_regex = regex::Regex::new(r"(?m)^def(\s+(\S+)\s*:?=\s*([^;\n]+)\s*;?)?")
        .expect("regex compile error");
    let repeat_regex = regex::Regex::new(r"(?m)^\s*(repeat|rep)\s+(0*[1-9][0-9]*)\s*:\s*([^\n]*)$")
        .expect("regex compile error");

    for file in dst.as_ref().read_dir().or_else(|e| {
        Err(ErrorKind::FailedToRead(
            dst.as_ref().to_owned(),
            "preprocessing stage: dir: remove".to_string(),
            e,
        ))
    })? {
        let file = file
            .or_else(|err| {
                Err(ErrorKind::FailedToRead(
                    dst.as_ref().to_owned(),
                    "preprocessing stage: dir content: remove".to_string(),
                    err,
                ))
            })?
            .path();

        if file
            .extension()
            .map(|o| o.to_string_lossy().to_string())
            .unwrap_or("".to_string())
            == "u"
        {
            if let Err(e) = std::fs::remove_file(&file) {
                return Err(ErrorKind::FileDeletionError(
                    file.clone(),
                    "preprocessing stage".to_string(),
                    e,
                )
                .into());
            }
        }
    }

    for file in src.as_ref().read_dir().or_else(|e| {
        Err(ErrorKind::FailedToRead(
            dst.as_ref().to_owned(),
            "preprocessing stage: dir: compile".to_string(),
            e,
        ))
    })? {
        let file = file
            .or_else(|err| {
                Err(ErrorKind::FailedToRead(
                    dst.as_ref().to_owned(),
                    "preprocessing stage: dir content: compile".to_string(),
                    err,
                ))
            })?
            .path();

        if !file.is_temporary() {
            println!("cargo:rerun-if-changed={}", file.to_string_lossy());
        }

        if file
            .extension()
            .map(|o| o.to_string_lossy().to_string())
            .unwrap_or("".to_string())
            != "u"
        {
            continue;
        }

        let content = std::fs::read_to_string(&file).or_else(|e| {
            Err::<String, Error>(
                ErrorKind::FailedToRead(file.clone(), "preprocessing stage: read".to_string(), e)
                    .into(),
            )
        })?;

        const MAX_ITERATIONS: usize = 10;

        let mut target_content = content.clone();
        for i in 0..MAX_ITERATIONS + 1 {
            let content_before = target_content.clone();

            target_content = repeat_regex
                .replace_all(&target_content, RepeatReplacer::default())
                .to_string();
            target_content = include_regex
                .replace_all(&target_content, IncludeReplacer::new(&cwd))
                .to_string();
            target_content = func_include_regex
                .replace_all(&target_content, FuncIncludeReplacer::new(&cwd))
                .to_string();

            if content_before == target_content {
                break;
            }

            if i == MAX_ITERATIONS {
                return Err(ErrorKind::PreprocessorError(
                    file.clone(),
                    "maximum iterations reached. lopped include?".to_string(),
                )
                .into());
            }
        }

        if target_content.contains(ERROR_MARKER) {
            let error_description = target_content
                .split(ERROR_MARKER)
                .last()
                .unwrap()
                .lines()
                .next()
                .unwrap();
            return Err(
                ErrorKind::PreprocessorError(file.clone(), error_description.to_string()).into(),
            );
        }

        let mut define_replacer = DefineResolveReplacer::default();
        target_content = define_regex
            .replace_all(&target_content, &mut define_replacer)
            .to_string();

        let defines = define_replacer.defines;
        target_content = defines.iter().fold(target_content, |acc, (name, value)| {
            acc.replace(name, value)
        });

        let target_file = dst
            .as_ref()
            .join(file.file_name().expect("file name error"));

        std::fs::write(&target_file, target_content).or_else(|err| {
            Err::<(), Error>(
                ErrorKind::FailedToWrite(
                    target_file.clone(),
                    "preprocessing stage: write".to_string(),
                    err,
                )
                .into(),
            )
        })?;
    }

    Ok(())
}

trait IsTemporaryPathExt {
    fn is_temporary(&self) -> bool;
}

impl<T: AsRef<Path>> IsTemporaryPathExt for T {
    fn is_temporary(&self) -> bool {
        let first = self.as_ref().iter().take(2).collect::<Vec<&OsStr>>();
        if first.len() < 2 {
            return false;
        }

        first[0] == "/" && first[1] == "tmp"
    }
}