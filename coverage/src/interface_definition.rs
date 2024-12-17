pub struct ComInterfaceDescription {
    pub base: u16,
    pub max_number_of_hooks: u8,
    pub offset_coverage_result_table: usize,
    pub offset_jump_back_table: usize,
    pub offset_instruction_table: usize,
    pub offset_clock_table_settings: usize,
    pub offset_clock_table: usize,
}

impl ComInterfaceDescription {
    /// Memory usage in bytes
    #[allow(dead_code)] // todo: maybe cargo error, since it is actually used, investigate and if valid, report to cargo
    pub const fn memory_usage(&self) -> usize {
        let jump = self.offset_jump_back_table + JUMP_TABLE_BYTE_SIZE;
        let coverage = self.offset_coverage_result_table + COVERAGE_RESULT_TABLE_BYTE_SIZE;
        let instructions = self.offset_instruction_table + INSTRUCTION_TABLE_BYTE_SIZE;
        let clocks = self.offset_clock_table + CLOCK_TABLE_BYTE_SIZE;
        let clocks_settings = self.offset_clock_table_settings + CLOCK_TABLE_SETTINGS_BYTE_SIZE;

        const fn max(a: usize, b: usize) -> usize {
            if a > b {
                a
            } else {
                b
            }
        }

        max(
            max(jump, coverage),
            max(instructions, max(clocks, clocks_settings)),
        )
    }
}

const MAX_NUMBER_OF_HOOKS: u8 = 8;

pub type CoverageEntry = u16;
const COVERAGE_RESULT_TABLE_BYTE_SIZE: usize =
    size_of::<CoverageEntry>() * MAX_NUMBER_OF_HOOKS as usize * 2;

pub type JumpTableEntry = u16;
const JUMP_TABLE_BYTE_SIZE: usize = size_of::<JumpTableEntry>() * MAX_NUMBER_OF_HOOKS as usize;

pub type InstructionTableEntry = [u64; 4];
const INSTRUCTION_TABLE_BYTE_SIZE: usize =
    size_of::<InstructionTableEntry>() * MAX_NUMBER_OF_HOOKS as usize * 4;

pub type ClockTableEntry = u64;
const CLOCK_TABLE_BYTE_SIZE: usize =
    size_of::<ClockTableEntry>() * MAX_NUMBER_OF_HOOKS as usize * 2;

pub type ClockTableSettingsEntry = CoverageEntry;
const CLOCK_TABLE_SETTINGS_BYTE_SIZE: usize =
    size_of::<ClockTableSettingsEntry>() * MAX_NUMBER_OF_HOOKS as usize * 2;

pub const COM_INTERFACE_DESCRIPTION: ComInterfaceDescription = ComInterfaceDescription {
    base: 0x1000,
    max_number_of_hooks: MAX_NUMBER_OF_HOOKS,
    offset_coverage_result_table: 0,
    offset_jump_back_table: COVERAGE_RESULT_TABLE_BYTE_SIZE,
    offset_instruction_table: COVERAGE_RESULT_TABLE_BYTE_SIZE + JUMP_TABLE_BYTE_SIZE,
    offset_clock_table: COVERAGE_RESULT_TABLE_BYTE_SIZE
        + JUMP_TABLE_BYTE_SIZE
        + INSTRUCTION_TABLE_BYTE_SIZE,
    offset_clock_table_settings: COVERAGE_RESULT_TABLE_BYTE_SIZE
        + JUMP_TABLE_BYTE_SIZE
        + INSTRUCTION_TABLE_BYTE_SIZE
        + CLOCK_TABLE_BYTE_SIZE,
};
