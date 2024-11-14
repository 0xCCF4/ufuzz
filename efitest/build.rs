fn main() {
    println!("cargo::rerun-if-changed=build.rs");

    ucode_compiler::compiler_call::build_script("patches", "src/patches", true);
}
