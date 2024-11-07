use std::path::{Path, PathBuf};
use regex::{Captures, Replacer};

fn main() {
    println!("cargo::rerun-if-changed=build.rs");

    if !std::fs::exists("src/patches").expect("fs perm denied") {
        std::fs::create_dir("src/patches").expect("dir creation failed")
    }

    assemble_scripts("patches", "src/patches");
    ucode_compiler::build_script("src/patches", "src/patches", true);
    delete_intermediate_files("src/patches");
}

fn assemble_scripts<A: AsRef<Path>, B: AsRef<Path>>(src: A, dst: B) {
    let include_regex = regex::Regex::new(r"\#include( <([^>]+)>)?").expect("regex compile error");
    #[derive(Default)]
    struct IncludeReplacer {}
    impl Replacer for IncludeReplacer {
        fn replace_append(&mut self, caps: &Captures<'_>, dst: &mut String) {
            let name = caps.get(2).expect("include path not given").as_str();
            let content = std::fs::read_to_string(PathBuf::from("patches").join(name)).expect("read failed");
            dst.push_str(&content);
        }
    }

    for file in dst.as_ref()
        .read_dir()
        .expect("Target directory not readable") {

        let file = file.expect("Target file not found").path();

        if file.extension().map(|o| o.to_string_lossy().to_string()).unwrap_or("".to_string()) == "u" {
            std::fs::remove_file(file).unwrap()
        }
    }

    for file in src
        .as_ref()
        .read_dir()
        .expect("Source directory not readable")
    {
        let file = file.expect("Target file not found").path();

        println!("cargo:rerun-if-changed={}", file.to_string_lossy());

        if file.extension().map(|o| o.to_string_lossy().to_string()).unwrap_or("".to_string()) != "u" {
            continue;
        }

        let content = std::fs::read_to_string(&file).expect("read failed");

        let target_content = include_regex.replace_all(&content, IncludeReplacer::default()).to_string();

        let target_file = dst.as_ref().join(file.file_name().expect("file name error"));

        std::fs::write(target_file, target_content).expect("write failed");
    }
}

fn delete_intermediate_files<A: AsRef<Path>>(path: A) {
    for file in path
        .as_ref()
        .read_dir()
        .expect("Intermediate directory not readable")
    {
        let file = file.expect("Intermediate file not found").path();

        if file.extension().map(|o| o.to_string_lossy().to_string()).unwrap_or("".to_string()) == "u" {
            std::fs::remove_file(file).expect("remove failed");
        }
    }
}