use std::path::{Path};

fn main() {
    println!("cargo::rerun-if-changed=build.rs");

    if !std::fs::exists("src/patches").expect("fs perm denied") {
        std::fs::create_dir("src/patches").expect("dir creation failed")
    }

    ucode_compiler::preprocess_scripts("patches", "src/patches");
    ucode_compiler::build_script("src/patches", "src/patches", true);
    // delete_intermediate_files("src/patches");
}

#[allow(dead_code)]
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