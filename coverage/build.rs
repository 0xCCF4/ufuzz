use crate::build::ucode_scripts;

#[path = "src/interface_definition.rs"]
mod interface_definition;
mod build {
    pub mod static_jump_analysis;
    pub mod ucode_scripts;
}

fn main() {
    #[cfg(feature = "ucode")]
    {
        println!("cargo::rerun-if-changed=build.rs");
        println!("cargo::rerun-if-changed=build/static_jump_analysis.rs");
        println!("cargo::rerun-if-changed=build/ucode_scripts.rs");

        // jump_analysis();
        if let Err(err) = ucode_scripts::build_ucode_scripts() {
            panic!("Failed to compile ucode scripts: {:?}", err);
        }
    }
}
