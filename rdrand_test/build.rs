fn main() {
    println!("cargo::rerun-if-changed=build.rs");

    ucode_compiler::build_script("patches", "src/patches", true);
}
