use std::path::{Path};
use ucode_compiler::AUTOGEN;

#[path = "src/interface_definition.rs"]
mod interface_definition;

fn main() {
    println!("cargo::rerun-if-changed=build.rs");

    if !std::fs::exists("src/patches").expect("fs perm denied") {
        std::fs::create_dir("src/patches").expect("dir creation failed")
    }

    if !std::fs::exists("patches/gen").expect("fs perm denied") {
        std::fs::create_dir("patches/gen").expect("dir creation failed")
    }

    generate_ucode_files("patches/gen");
    ucode_compiler::preprocess_scripts("patches", "src/patches");
    ucode_compiler::build_script("src/patches", "src/patches", true);
    // delete_intermediate_files("src/patches");
}

#[allow(dead_code)]
fn delete_intermediate_files<A: AsRef<Path>>(path: A) {
    for file in path
        .as_ref()
        .read_dir()
        .expect("Intermediate directory not readable")
    {
        let file = file.expect("Intermediate file not found").path();

        if file
            .extension()
            .map(|o| o.to_string_lossy().to_string())
            .unwrap_or("".to_string())
            == "u"
        {
            std::fs::remove_file(file).expect("remove failed");
        }
    }
}

fn generate_ucode_files<A: AsRef<Path>>(path: A) {
    let file = path.as_ref().join("interface_definition.up");

    let interface = interface_definition::COM_INTERFACE_DESCRIPTION;
    let mut definitions = Vec::default();

    fn u16_to_u8_addr(addr: usize) -> usize {
        addr << 1
    }

    definitions.push(("index_mask", 2usize.pow((interface.max_number_of_hooks + 1).ilog2())-1, "mask to apply to an index to get an index in the range of the tables"));
    definitions.push(("address_base", interface.base, "base address of the interface"));
    definitions.push(("address_coverage_table_base", interface.base + u16_to_u8_addr(interface.offset_coverage_result_table), "base address of the coverage table"));
    definitions.push(("address_jump_table_base", interface.base + u16_to_u8_addr(interface.offset_jump_back_table), "base address of the jump table"));
    definitions.push(("table_length", interface.max_number_of_hooks, "number of entries in the tables"));
    definitions.push(("table_size", u16_to_u8_addr(interface.max_number_of_hooks), "size of the tables in bytes"));

    let definitions = definitions.iter().map(|(name, value, comment)|
        format!("# {comment}\ndef [{name}] = 0x{value:04x};")).collect::<Vec<_>>().join("\n\n");

    std::fs::write(file, format!("# {AUTOGEN}\n\n{definitions}").as_str()).expect("write failed");
}