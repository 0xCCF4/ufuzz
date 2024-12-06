use crate::build::msrom::build_ms_rom;
use crate::build::ucode_scripts;

#[path = "src/interface_definition.rs"]
mod interface_definition;
mod build {
    pub mod msrom;
    pub mod ucode_scripts;
}

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=build/msrom.rs");
    println!("cargo::rerun-if-changed=build/ucode_scripts.rs");

    if let Err(err) = build_ms_rom() {
        panic!("Failed to parse ucode rom files: {:?}", err);
    }
    if let Err(err) = ucode_scripts::build_ucode_scripts() {
        panic!("Failed to compile ucode scripts: {:?}", err);
    }
}
