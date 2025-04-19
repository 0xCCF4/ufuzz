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
