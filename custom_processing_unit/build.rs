use ucode_compiler_bridge::CompilerOptions;

fn main() {
    println!("cargo::rerun-if-changed=build.rs");

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
