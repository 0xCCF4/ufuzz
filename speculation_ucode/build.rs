//! Build script for the speculation_ucode crate
//!
//! This script is responsible for compiling microcode patches during the build process.
//! It uses the `ucode_compiler_bridge` to compile microcode source files into Rust
//! modules that can be used by the crate.
//!

use ucode_compiler_bridge::CompilerOptions;

fn main() {
    println!("cargo::rerun-if-changed=build.rs");

    let options = CompilerOptions {
        allow_unused: false,
        cpuid: None,
        avoid_unknown_256: false,
    };
    ucode_compiler_bridge::preprocess_scripts("patches", "src/patches", "patches")
        .expect("Failed to compile microcode");
    ucode_compiler_bridge::compile_source_and_create_module("src/patches", "src/patches", &options)
        .expect("Failed to compile microcode");
}
