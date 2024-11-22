use std::path::Path;
use ucode_compiler::uasm::AUTOGEN;

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

    // STAGE 1

    generate_ucode_files("patches/gen", None);
    ucode_compiler::uasm::preprocess_scripts("patches", "src/patches");
    ucode_compiler::uasm::build_script("src/patches", "src/patches", true);

    let regex_hook_entry = regex::Regex::new(r"LABEL_HOOK_ENTRY_00:[^(]+\(0x([^)]+)\)").expect("regex failed");
    let patch_text = std::fs::read_to_string("src/patches/patch.rs").expect("read failed");
    let hook_entry = regex_hook_entry.captures(patch_text.as_str()).expect("hook entry label not found").get(1).expect("hook entry not found").as_str();

    // STAGE 2

    generate_ucode_files("patches/gen", Some(usize::from_str_radix(hook_entry, 16).expect("hook entry not a number")));
    ucode_compiler::uasm::preprocess_scripts("patches", "src/patches");
    ucode_compiler::uasm::build_script("src/patches", "src/patches", true);

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

fn generate_ucode_files<A: AsRef<Path>>(path: A, hook_entry: Option<usize>) {
    let file = path.as_ref().join("interface_definition.up");

    let interface = interface_definition::COM_INTERFACE_DESCRIPTION;
    let mut definitions = Vec::default();

    fn u16_to_u8_addr(addr: usize) -> usize {
        addr << 1
    }

    definitions.push((
        "index_mask",
        2usize.pow((interface.max_number_of_hooks + 1).ilog2()) - 1,
        "mask to apply to an index to get an index in the range of the tables",
    ));
    definitions.push((
        "offset_mask",
        (2usize.pow((interface.max_number_of_hooks + 1).ilog2()) - 1) << 1,
        "mask to apply to an offset into the tables to get a valid u16 ptr",
    ));
    definitions.push((
        "address_base",
        interface.base,
        "base address of the interface",
    ));
    definitions.push((
        "address_coverage_table_base",
        interface.base + u16_to_u8_addr(interface.offset_coverage_result_table),
        "base address of the coverage table",
    ));
    definitions.push((
        "address_jump_table_base",
        interface.base + u16_to_u8_addr(interface.offset_jump_back_table),
        "base address of the jump table",
    ));
    definitions.push((
        "table_length",
        interface.max_number_of_hooks,
        "number of entries in the tables",
    ));
    definitions.push((
        "table_size",
        u16_to_u8_addr(interface.max_number_of_hooks),
        "size of the tables in bytes",
    ));
    //assert!(interface.offset_timing_table > interface.offset_coverage_result_table);
    //assert!(interface.offset_jump_back_table > interface.offset_timing_table);
    assert!(interface.offset_jump_back_table > interface.offset_coverage_result_table);
    // definitions.push(("offset_cov2time_table", u16_to_u8_addr(interface.offset_timing_table - interface.offset_coverage_result_table), "offset between the coverage and timing tables"));
    // definitions.push(("offset_time2jump_table", u16_to_u8_addr(interface.offset_jump_back_table - interface.offset_timing_table), "offset between the timing and jump tables"));
    definitions.push((
        "offset_cov2jump_table",
        u16_to_u8_addr(interface.offset_jump_back_table - interface.offset_coverage_result_table),
        "offset between the coverage and jump tables",
    ));
    definitions.push((
        "hook_entry_address",
        hook_entry.unwrap_or(0),
        "address of the first hook entry point"
    ));
    definitions.push((
        "hook_entry_address_offset",
        hook_entry.unwrap_or(0x7c00)-0x7c00,
        "address of the first hook entry point"
    ));

    let definitions = definitions
        .iter()
        .map(|(name, value, comment)| format!("# {comment}\ndef [{name}] = 0x{value:04x};"))
        .collect::<Vec<_>>()
        .join("\n\n");

    std::fs::write(file, format!("# {AUTOGEN}\n\n{definitions}").as_str()).expect("write failed");

    let file = path.as_ref().join("entries.up");
    let mut content = "".to_string();
    content.push_str("# ");
    content.push_str(AUTOGEN);
    content.push_str("\n\n");

    for i in 0..interface.max_number_of_hooks {
        let val = u16_to_u8_addr(i);
        content.push_str(
            format!(
                "
<hook_entry_{i:02}>
NOP SEQW SYNCFULL
STADSTGBUF_DSZ64_ASZ16_SC1([adr_stg_r10], , r10) !m2  # save original value of r10
[in_hook_offset] := ZEROEXT_DSZ32(0x{val:02x}) SEQW GOTO <handler> # hook index {i} -> offset {val}
"
            )
            .as_str(),
        )
    }

    std::fs::write(file, content).expect("write failed");

    let file = path.as_ref().join("exits.up");
    let mut content = "".to_string();
    content.push_str("# ");
    content.push_str(AUTOGEN);
    content.push_str("\n\n");

    for i in 0..interface.max_number_of_hooks {
        let val = u16_to_u8_addr(i);
        content.push_str(
            format!(
                "
<hook_exit_{i:02}>
r10 := LDSTGBUF_DSZ64_ASZ16_SC1([adr_stg_r10]) !m2
NOP SEQW SYNCFULL
NOP SEQW GOTO <exit_trap> # will be overriden when a new hook is set
"
            )
                .as_str(),
        )
    }

    std::fs::write(file, content).expect("write failed");
}
