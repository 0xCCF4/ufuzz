//! Build script for the custom_processing_unit crate
//! 
//! This script is responsible for compiling microcode patches during the build process.
//! It uses the `ucode_compiler_bridge` to compile microcode source files into Rust
//! modules that can be used by the crate.
//! 

use ucode_compiler_bridge::CompilerOptions;

fn main() {
    // Ensure the build script reruns if modified
    println!("cargo::rerun-if-changed=build.rs");

    // Compile microcode patches and create Rust modules
    if let Err(err) = ucode_compiler_bridge::compile_source_and_create_module(
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
}
