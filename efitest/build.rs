fn main() {
    println!("cargo::rerun-if-changed=build.rs");

    if let Err(err) =
        ucode_compiler::uasm::compile_source_and_create_module("patches", "src/patches", true)
    {
        panic!("Failed to compile: {:?}", err);
    }
}
