//! This module defines the core interface structures for coverage analysis.
//! It defines memory layout and table structures for storing coverage data.

use core::mem::align_of;
use core::mem::size_of;

/// Describes the communication interface layout and configuration
/// 
/// This structure defines the memory layout for coverage data collection,
/// including table offsets and size limits.
#[allow(dead_code)]
pub struct ComInterfaceDescription {
    /// Base address for the interface in memory
    pub base: u16,
    /// Maximum number of hooks that can be installed
    pub max_number_of_hooks: usize,
    /// Offset to the coverage result table from base
    pub offset_coverage_result_table: usize,
    /// Offset to the jump back table from base
    pub offset_jump_back_table: usize,
    /// Offset to the instruction table from base
    pub offset_instruction_table: usize,
    /// Offset to the last RIP table from base
    pub offset_last_rip_table: usize,
}

impl ComInterfaceDescription {
    /// Calculates total memory usage in bytes
    /// 
    /// # Returns
    /// 
    /// Total size of all tables and data structures
    #[allow(dead_code)] // todo: maybe cargo error, since it is actually used, investigate and if valid, report to cargo
    pub const fn memory_usage(&self) -> usize {
        let jump = self.offset_jump_back_table + JUMP_TABLE_BYTE_SIZE;
        let coverage = self.offset_coverage_result_table + COVERAGE_RESULT_TABLE_BYTE_SIZE;
        let instructions = self.offset_instruction_table + INSTRUCTION_TABLE_BYTE_SIZE;
        let rips = self.offset_last_rip_table + LAST_RIP_TABLE_BYTE_SIZE;
        const fn max(a: usize, b: usize) -> usize {
            if a > b {
                a
            } else {
                b
            }
        }

        let size = max(max(jump, coverage), max(instructions, rips));
        assert!(size < 4096, "Todo");
        size
    }

    /// Checks if any tables overlap in memory
    #[allow(dead_code)]
    pub fn check_overlap(&self) -> bool {
        let start_jump_table = self.offset_jump_back_table;
        let end_jump_table = start_jump_table + JUMP_TABLE_BYTE_SIZE - 1;

        let start_coverage_table = self.offset_coverage_result_table;
        let end_coverage_table = start_coverage_table + COVERAGE_RESULT_TABLE_BYTE_SIZE - 1;

        let start_instruction_table = self.offset_instruction_table;
        let end_instruction_table = start_instruction_table + INSTRUCTION_TABLE_BYTE_SIZE - 1;

        let start_last_rip_table = self.offset_last_rip_table;
        let end_last_rip_table = start_last_rip_table + LAST_RIP_TABLE_BYTE_SIZE - 1;

        let pairs = [
            (start_jump_table, end_jump_table),
            (start_coverage_table, end_coverage_table),
            (start_instruction_table, end_instruction_table),
            (start_last_rip_table, end_last_rip_table),
        ];

        for i in 0..pairs.len() {
            for j in 0..pairs.len() {
                if i == j {
                    continue;
                }

                let (start1, end1) = pairs[i];
                let (start2, end2) = pairs[j];

                if start1 <= end2 && end1 >= start2 {
                    //panic!("Overlap detected between tables {} and {}: {:04x}-{:04x} and {:04x}-{:04x}", i, j, start1, end1, start2, end2);
                    return false;
                }
            }
        }

        true
    }
}

/// Maximum number of hooks that can be installed
/// 
/// This sets an upper limit on coverage collection capacity.
const MAX_NUMBER_OF_HOOKS: usize = 1; // todo: change back to higher value

/// Type for storing coverage count values
pub type CoverageCount = u16;

/// Entry in the coverage results table
/// 
/// Contains two counters since collection is done in pairs.
pub type CoverageEntry = [CoverageCount; 2];

/// Total size of the coverage result table in bytes
pub const COVERAGE_RESULT_TABLE_BYTE_SIZE: usize = size_of::<CoverageEntry>() * MAX_NUMBER_OF_HOOKS;

/// Entry in the jump table
/// 
/// Stores addresses for hook return points.
pub type JumpTableEntry = u16;

/// Total size of the jump table in bytes
pub const JUMP_TABLE_BYTE_SIZE: usize = size_of::<JumpTableEntry>() * MAX_NUMBER_OF_HOOKS;

/// Entry in the instruction table
/// 
/// Stores instruction triads overwritten by hooks. Two triads per hook.
pub type InstructionTableEntry = [[u64; 4]; 2];

/// Total size of the instruction table in bytes
pub const INSTRUCTION_TABLE_BYTE_SIZE: usize =
    size_of::<InstructionTableEntry>() * MAX_NUMBER_OF_HOOKS;

/// Entry in the last RIP table
/// 
/// Stores the last instruction pointer values for each hook.
pub type LastRIPEntry = [u64; 2];

/// Total size of the last RIP table in bytes
pub const LAST_RIP_TABLE_BYTE_SIZE: usize = size_of::<LastRIPEntry>() * MAX_NUMBER_OF_HOOKS;

/// Start offset of the coverage table
#[allow(dead_code)]
const START_OF_COVERAGE_TABLE: usize = 0;

/// End offset of the coverage table
#[allow(dead_code)]
const END_OF_COVERAGE_TABLE: usize = COVERAGE_RESULT_TABLE_BYTE_SIZE;

/// Genuine (unaligned) start offset of the jump table
#[allow(dead_code)]
const GENUINE_START_OF_JUMP_TABLE: usize = END_OF_COVERAGE_TABLE;

/// Aligned start offset of the jump table
#[allow(dead_code)]
const START_OF_JUMP_TABLE: usize = align(align_of::<JumpTableEntry>(), GENUINE_START_OF_JUMP_TABLE);

/// End offset of the jump table
#[allow(dead_code)]
const END_OF_JUMP_TABLE: usize = START_OF_JUMP_TABLE + JUMP_TABLE_BYTE_SIZE;

/// Genuine (unaligned) start offset of the instruction table
#[allow(dead_code)]
const GENUINE_START_OF_INSTRUCTION_TABLE: usize = END_OF_JUMP_TABLE;

/// Aligned start offset of the instruction table
#[allow(dead_code)]
const START_OF_INSTRUCTION_TABLE: usize = align(
    align_of::<InstructionTableEntry>(),
    GENUINE_START_OF_INSTRUCTION_TABLE,
);

/// End offset of the instruction table
#[allow(dead_code)]
const END_OF_INSTRUCTION_TABLE: usize = START_OF_INSTRUCTION_TABLE + INSTRUCTION_TABLE_BYTE_SIZE;

/// Genuine (unaligned) start offset of the last RIP table
#[allow(dead_code)]
const GENUINE_START_OF_LAST_RIP_TABLE: usize = END_OF_INSTRUCTION_TABLE;

/// Aligned start offset of the last RIP table
#[allow(dead_code)]
const START_OF_LAST_RIP_TABLE: usize =
    align(align_of::<LastRIPEntry>(), GENUINE_START_OF_LAST_RIP_TABLE);
//const END_OF_LAST_RIP_TABLE:usize = START_OF_LAST_RIP_TABLE+LAST_RIP_TABLE_BYTE_SIZE;

/// Default interface description with predefined memory layout
#[allow(dead_code)]
pub const COM_INTERFACE_DESCRIPTION: ComInterfaceDescription = ComInterfaceDescription {
    base: 0x1000,
    max_number_of_hooks: MAX_NUMBER_OF_HOOKS,
    offset_coverage_result_table: START_OF_COVERAGE_TABLE,
    offset_jump_back_table: START_OF_JUMP_TABLE,
    offset_instruction_table: START_OF_INSTRUCTION_TABLE,
    offset_last_rip_table: START_OF_LAST_RIP_TABLE,
};

/// Calculates alignment correction for a value (pow2)
/// 
/// # Arguments
/// 
/// * `to` - Alignment requirement (power of 2)
/// * `value` - Value to align
/// 
/// # Returns
/// 
/// Number of bytes needed to align the value
const fn align_correction(to: usize, value: usize) -> usize {
    let mask: usize = 2 << to;
    mask - value % mask
}

/// Aligns a value to a specified boundary (pow2)
/// 
/// # Arguments
/// 
/// * `to` - Alignment requirement (power of 2)
/// * `value` - Value to align
/// 
/// # Returns
/// 
/// Aligned value
const fn align(to: usize, value: usize) -> usize {
    value + align_correction(to, value)
}
