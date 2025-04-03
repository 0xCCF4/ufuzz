use crate::interface_definition;
use crate::interface_definition::{
    ComInterfaceDescription, CoverageEntry, InstructionTableEntry, JumpTableEntry,
};
use core::mem::size_of;
use std::path::Path;
use ucode_compiler_bridge::{CompilerOptions, AUTOGEN};
use crate::interface_definition::LastRIPEntry;

pub fn build_ucode_scripts() -> ucode_compiler_bridge::Result<()> {
    if !Path::new("src/patches").exists() {
        std::fs::create_dir("src/patches").expect("dir creation failed")
    }

    if !Path::new("patches/gen").exists() {
        std::fs::create_dir("patches/gen").expect("dir creation failed")
    }

    let options = CompilerOptions {
        allow_unused: true,
        avoid_unknown_256: true,
        cpuid: None,
    };

    // STAGE 1

    generate_ucode_files(
        "patches/gen",
        None,
        &interface_definition::COM_INTERFACE_DESCRIPTION,
    );
    ucode_compiler_bridge::preprocess_scripts("patches", "src/patches", "patches")?;
    ucode_compiler_bridge::compile_source_and_create_module(
        "src/patches",
        "src/patches",
        &options,
    )?;

    let result_text = std::fs::read_to_string("src/patches/patch.rs").expect("read failed");

    // STAGE 2

    let data = Stage2Pass::from(result_text.as_str());

    generate_ucode_files(
        "patches/gen",
        Some(data),
        &interface_definition::COM_INTERFACE_DESCRIPTION,
    );
    ucode_compiler_bridge::preprocess_scripts("patches", "src/patches", "patches")?;
    ucode_compiler_bridge::compile_source_and_create_module(
        "src/patches",
        "src/patches",
        &options,
    )?;

    // delete_intermediate_files("src/patches");

    Ok(())
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

#[derive(Copy, Clone)]
struct Stage2Pass {
    hook_entry_address: usize,
    hook_exit_address: usize,
}

impl Default for Stage2Pass {
    fn default() -> Self {
        Stage2Pass {
            hook_entry_address: 0x7c00,
            hook_exit_address: 0x7c00,
        }
    }
}

#[track_caller]
fn extract_label_value(text: &str, label: &str) -> Option<usize> {
    let regex = regex::Regex::new(format!(r"LABEL_{}:[^(]+\(0x([^)]+)\)", label).as_str())
        .expect("regex failed");
    let hook_entry = regex
        .captures(text)?
        .get(1)
        .expect("hook entry not found")
        .as_str();
    Some(usize::from_str_radix(hook_entry, 16).expect("hook entry not a number"))
}

impl From<&str> for Stage2Pass {
    fn from(value: &str) -> Self {
        let hook_entry_address = extract_label_value(value, "HOOK_ENTRY_00")
            .expect("hook entry label <hook_entry_00> missing");
        let hook_exit_address = extract_label_value(value, "HOOK_EXIT_00")
            .expect("hook exit label <hook_exit_00> missing");

        Stage2Pass {
            hook_entry_address,
            hook_exit_address,
        }
    }
}

fn generate_ucode_files<A: AsRef<Path>>(
    path: A,
    pass1data: Option<Stage2Pass>,
    interface: &ComInterfaceDescription,
) {
    generate_interface_definitions(path.as_ref(), pass1data, interface);
    //generate_entries(path.as_ref(), interface);
    //generate_exits(path.as_ref(), interface, pass1data);
    generate_patch(path.as_ref(), interface);
}

#[allow(clippy::vec_init_then_push)]
fn generate_interface_definitions<A: AsRef<Path>>(
    path: A,
    pass1data: Option<Stage2Pass>,
    interface: &ComInterfaceDescription,
) {
    let file = path.as_ref().join("interface_definition.up");

    let mut definitions = Vec::default();

    if let Some(data) = pass1data {
        assert!(data.hook_entry_address > 0x7c00);
        assert!(data.hook_exit_address > 0x7c00);
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
        interface.base as usize,
        "base address of the interface",
    ));
    definitions.push((
        "address_coverage_table_base",
        interface.base as usize + interface.offset_coverage_result_table,
        "base address of the coverage table",
    ));
    definitions.push((
        "address_jump_table_base",
        interface.base as usize + interface.offset_jump_back_table,
        "base address of the jump table",
    ));
    definitions.push((
        "address_instruction_table_base",
        interface.base as usize + interface.offset_instruction_table,
        "base address of the instruction table",
    ));
    definitions.push((
        "address_lastrip_table_base",
        interface.base as usize + interface.offset_last_rip_table,
        "base address of the last rip table",
    ));
    definitions.push((
        "table_length",
        interface.max_number_of_hooks,
        "number of entries in the tables",
    ));
    definitions.push((
        "table_size_jump",
        interface.max_number_of_hooks * size_of::<JumpTableEntry>(),
        "size of the jump table in bytes",
    ));
    definitions.push((
        "table_size_coverage",
        interface.max_number_of_hooks * size_of::<CoverageEntry>(),
        "size of the coverage table in bytes",
    ));
    definitions.push((
        "table_size_instruction",
        interface.max_number_of_hooks * size_of::<InstructionTableEntry>(),
        "size of the instruction table in bytes",
    ));
    definitions.push((
        "size_jump_table_entry",
        size_of::<JumpTableEntry>(),
        "size of a jump table entry in bytes",
    ));
    definitions.push((
        "size_coverage_table_entry",
        size_of::<CoverageEntry>(),
        "size of a coverage table entry in bytes",
    ));
    definitions.push((
        "size_instruction_table_entry",
        size_of::<InstructionTableEntry>(),
        "size of a instruction table entry in bytes",
    ));
    assert!(size_of::<JumpTableEntry>().is_power_of_two());
    assert!(size_of::<CoverageEntry>().is_power_of_two());
    assert!(size_of::<InstructionTableEntry>().is_power_of_two());
    definitions.push((
        "convert_index_to_jump_table_offset",
        size_of::<JumpTableEntry>().ilog2() as usize - 1, // since two entries, make it prettier later
        "shift by this amount to get the offset in the jump table",
    ));
    definitions.push((
        "convert_index_to_coverage_table_offset",
        size_of::<CoverageEntry>().ilog2() as usize - 1, // since two entries, make it prettier later
        "shift by this amount to get the offset in the coverage table",
    ));
    definitions.push((
        "convert_index_to_instruction_table_offset",
        size_of::<InstructionTableEntry>().ilog2() as usize - 1, // since two entries, make it prettier later
        "shift by this amount to get the offset in the instruction table",
    ));
    assert!(interface.offset_jump_back_table > interface.offset_coverage_result_table);
    definitions.push((
        "convert_index_to_lastrip_table_offset",
        size_of::<LastRIPEntry>().ilog2() as usize - 1, // since two entries, make it prettier later
        "shift by this amount to get the offset in the last rip table",
    ));
    definitions.push((
        "offset_cov2jump_table",
        interface.offset_jump_back_table - interface.offset_coverage_result_table,
        "offset between the coverage and jump tables",
    ));
    definitions.push((
        "hook_entry_address",
        pass1data.unwrap_or_default().hook_entry_address,
        "address of the first hook entry point",
    ));
    definitions.push((
        "hook_entry_address_offset",
        pass1data.unwrap_or_default().hook_entry_address - 0x7c00,
        "address of the first hook entry point",
    ));
    definitions.push((
        "hook_exit_offset_in_seqw",
        (pass1data.unwrap_or_default().hook_exit_address - 0x7c00) / 4,
        "offset in sequence ram to exit SEQW",
    ));
    definitions.push((
        "hook_exit_offset",
        pass1data.unwrap_or_default().hook_exit_address - 0x7c00,
        "offset in sequence ram to exit",
    ));

    let definitions = definitions
        .iter()
        .map(|(name, value, comment)| format!("# {comment}\ndef [{name}] = 0x{value:04x};"))
        .collect::<Vec<_>>()
        .join("\n\n");

    std::fs::write(file, format!("# {AUTOGEN}\n\n{definitions}").as_str()).expect("write failed");
}

fn generate_patch<A: AsRef<Path>>(path: A, interface: &ComInterfaceDescription) {
    let file = path.as_ref().join("patch.up");
    let mut content = "".to_string();
    content.push_str("# ");
    content.push_str(AUTOGEN);

    let mut entries = String::new();
    let mut exits = String::new();
    let mut handlers = String::new();

    for i in 0..interface.max_number_of_hooks {
        let even_index = i * 2;
        let odd_index = even_index + 1;

        entries.push_str(
            format!(
                "
NOPB # Align to 4
<hook_entry_{i:02}>
UJMP(,<hook_handler_{i:02}_even>)
UJMP(,<hook_handler_{i:02}_odd>)
UJMP(,<exit_trap>) # not necessary? otherwise just NOP, since we need to align to 4
"
            )
            .as_str(),
        );
        handlers.push_str(
            format!(
                "
<hook_handler_{i:02}_even>
func handler_inst(0x{even_index:02x}, hook_handler_{i:02}_even)
UJMP(,<hook_exit_{i:02}_even>)

<hook_handler_{i:02}_odd>
func handler_inst(0x{odd_index:02x}, hook_handler_{i:02}_odd)
UJMP(,<hook_exit_{i:02}_odd>)
"
            )
            .as_str(),
        );
        exits.push_str(
            format!(
                "
NOPB # Align to 4
<hook_exit_{i:02}>
<hook_exit_{i:02}_even>
NOP
NOP
NOP SEQW NOP

<hook_exit_{i:02}_odd>
NOP
NOP
NOP SEQW NOP
"
            )
            .as_str(),
        )
    }

    content.push_str(entries.as_str());
    content.push_str(handlers.as_str());
    content.push_str(exits.as_str());

    std::fs::write(file, content).expect("write failed");
}

#[allow(dead_code)]
fn generate_entries<A: AsRef<Path>>(path: A, interface: &ComInterfaceDescription) {
    let file = path.as_ref().join("entries.up");
    let mut content = "".to_string();
    content.push_str("# ");
    content.push_str(AUTOGEN);
    content.push_str("\n\nNOPB # Align to 4\n\n");

    for i in 0..interface.max_number_of_hooks {
        let even = i * 2;
        let odd = even + 1;
        content.push_str(
            format!(
                "
<hook_entry_{i:02}>
NOP SEQW GOTO <hook_entry_{i:02}_even>
NOP # if odd then just NOP to _odd entry
NOP
<hook_entry_{i:02}_odd>
STADSTGBUF_DSZ64_ASZ16_SC1([adr_stg_r10], , r10) !m2 # save original value of r10
[in_hook_offset] := ZEROEXT_DSZ32(0x{odd:02x}) SEQW GOTO <handler>
NOP
<hook_entry_{i:02}_even>
STADSTGBUF_DSZ64_ASZ16_SC1([adr_stg_r10], , r10) !m2 # save original value of r10
[in_hook_offset] := ZEROEXT_DSZ32(0x{even:02x}) SEQW GOTO <handler>
NOP
"
            )
            .as_str(),
        )
    }

    std::fs::write(file, content).expect("write failed");
}

#[allow(dead_code)]
fn generate_exits<A: AsRef<Path>>(
    path: A,
    interface: &ComInterfaceDescription,
    _data: Option<Stage2Pass>,
) {
    let file = path.as_ref().join("exits.up");
    let mut content = "".to_string();
    content.push_str("# ");
    content.push_str(AUTOGEN);
    content.push_str("\n\n");

    content.push_str("NOPB # Align to 4\n\n");

    for i in 0..interface.max_number_of_hooks {
        content.push_str(
            format!(
                "
<hook_exit_{i:02}>
<hook_exit_{i:02}_even>
r10 := LDSTGBUF_DSZ64_ASZ16_SC1([adr_stg_r10]) !m2 SEQW GOTO <exit_trap>
NOP
NOP

NOP
NOP
NOP SEQW GOTO <exit_trap>

<hook_exit_{i:02}_odd>
r10 := LDSTGBUF_DSZ64_ASZ16_SC1([adr_stg_r10]) !m2 SEQW GOTO <exit_trap>
NOP
NOP

NOP
NOP
NOP SEQW GOTO <exit_trap>
"
            )
            .as_str(),
        )
    }

    std::fs::write(file, content).expect("write failed");
}
