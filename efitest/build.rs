fn main() {
    println!("cargo::rerun-if-changed=build.rs");

    ucode_compiler::uasm::build_script("patches", "src/patches", true);
}
