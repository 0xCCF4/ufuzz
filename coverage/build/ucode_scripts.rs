use crate::interface_definition;
use crate::interface_definition::{
    ClockTableEntry, ClockTableSettingsEntry, ComInterfaceDescription, CoverageEntry,
    InstructionTableEntry, JumpTableEntry,
};
use std::path::Path;
use ucode_compiler::uasm::{CompilerOptions, AUTOGEN};

pub fn build_ucode_scripts() -> ucode_compiler::uasm::Result<()> {
    if !std::fs::exists("src/patches").expect("fs perm denied") {
        std::fs::create_dir("src/patches").expect("dir creation failed")
    }

    if !std::fs::exists("patches/gen").expect("fs perm denied") {
        std::fs::create_dir("patches/gen").expect("dir creation failed")
    }

    #[track_caller]
    fn extract_label_value(text: &str, label: &str) -> usize {
        let regex = regex::Regex::new(format!(r"LABEL_{}:[^(]+\(0x([^)]+)\)", label).as_str())
            .expect("regex failed");
        let hook_entry = regex
            .captures(text)
            .expect("hook entry label not found")
            .get(1)
            .expect("hook entry not found")
            .as_str();
        usize::from_str_radix(hook_entry, 16).expect("hook entry not a number")
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
    ucode_compiler::uasm::preprocess_scripts("patches", "src/patches", "patches")?;
    ucode_compiler::uasm::compile_source_and_create_module("src/patches", "src/patches", &options)?;

    let result_text = std::fs::read_to_string("src/patches/patch.rs").expect("read failed");

    // STAGE 2

    let data = Stage2Pass {
        hook_entry_address: extract_label_value(result_text.as_str(), "HOOK_ENTRY_00"),
        hook_exit_address: extract_label_value(result_text.as_str(), "HOOK_EXIT_00"),
    };

    generate_ucode_files(
        "patches/gen",
        Some(data),
        &interface_definition::COM_INTERFACE_DESCRIPTION,
    );
    ucode_compiler::uasm::preprocess_scripts("patches", "src/patches", "patches")?;
    ucode_compiler::uasm::compile_source_and_create_module("src/patches", "src/patches", &options)?;

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

fn generate_ucode_files<A: AsRef<Path>>(
    path: A,
    pass1data: Option<Stage2Pass>,
    interface: &ComInterfaceDescription,
) {
    generate_interface_definitions(path.as_ref(), pass1data, interface);
    generate_entries(path.as_ref(), interface);
    generate_exits(path.as_ref(), interface, pass1data);
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
        "address_clock_table_base",
        interface.base as usize + interface.offset_clock_table,
        "base address of the clock table",
    ));
    definitions.push((
        "address_clock_settings_table_base",
        interface.base as usize + interface.offset_clock_table_settings,
        "base address of the clock settings table",
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
        "table_size_clock",
        interface.max_number_of_hooks * size_of::<ClockTableEntry>(),
        "size of the clock table in bytes",
    ));
    definitions.push((
        "table_size_clock_settings",
        interface.max_number_of_hooks * size_of::<ClockTableSettingsEntry>(),
        "size of the clock settings table in bytes",
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
    definitions.push((
        "size_clock_table_entry",
        size_of::<ClockTableEntry>(),
        "size of a clock table entry in bytes",
    ));
    definitions.push((
        "size_clock_table_settings_entry",
        size_of::<ClockTableSettingsEntry>(),
        "size of a clock settings table entry in bytes",
    ));
    assert!(size_of::<JumpTableEntry>().is_power_of_two());
    assert!(size_of::<CoverageEntry>().is_power_of_two());
    assert!(size_of::<InstructionTableEntry>().is_power_of_two());
    assert!(size_of::<ClockTableEntry>().is_power_of_two());
    assert!(size_of::<ClockTableSettingsEntry>().is_power_of_two());
    definitions.push((
        "convert_index_to_jump_table_offset",
        size_of::<JumpTableEntry>().ilog2() as usize,
        "shift by this amount to get the offset in the jump table",
    ));
    definitions.push((
        "convert_index_to_coverage_table_offset",
        size_of::<CoverageEntry>().ilog2() as usize,
        "shift by this amount to get the offset in the coverage table",
    ));
    definitions.push((
        "convert_index_to_instruction_table_offset",
        size_of::<InstructionTableEntry>().ilog2() as usize,
        "shift by this amount to get the offset in the instruction table",
    ));
    definitions.push((
        "convert_index_to_clock_table_offset",
        size_of::<ClockTableEntry>().ilog2() as usize,
        "shift by this amount to get the offset in the clock table",
    ));
    definitions.push((
        "convert_index_to_clock_settings_table_offset",
        size_of::<ClockTableSettingsEntry>().ilog2() as usize,
        "shift by this amount to get the offset in the clock settings table",
    ));
    //assert!(interface.offset_timing_table > interface.offset_coverage_result_table);
    //assert!(interface.offset_jump_back_table > interface.offset_timing_table);
    assert!(interface.offset_jump_back_table > interface.offset_coverage_result_table);
    // definitions.push(("offset_cov2time_table", u16_to_u8_addr(interface.offset_timing_table - interface.offset_coverage_result_table), "offset between the coverage and timing tables"));
    // definitions.push(("offset_time2jump_table", u16_to_u8_addr(interface.offset_jump_back_table - interface.offset_timing_table), "offset between the timing and jump tables"));
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
