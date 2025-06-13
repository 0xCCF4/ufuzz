//! Microcode Instruction Module
//!
//! This module provides functionality for working with individual microcode instructions.
//! It includes tools for assembling, disassembling, and manipulating microcode instructions.

use crate::even_odd_parity_u64;
use crate::opcodes::Opcode;
use core::fmt::Display;
use data_types::addresses::{Address, UCInstructionAddress};
use num_traits::FromPrimitive;
use serde::{Deserialize, Serialize};

/// A microcode instruction
///
/// This type represents a single microcode instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Ord, PartialOrd)]
#[repr(transparent)]
pub struct Instruction {
    /// The raw instruction value
    instruction: u64,
}

impl Instruction {
    /// A no-operation instruction
    pub const NOP: Instruction = Instruction::disassemble(0);

    /// Creates a new instruction from a raw microcode operation value
    ///
    /// # Arguments
    ///
    /// * `micro_operation` - The raw 64-bit microcode operation value
    ///
    /// # Returns
    ///
    /// Returns a new Instruction with the CRC bits masked off
    pub const fn disassemble(micro_operation: u64) -> Instruction {
        Instruction {
            instruction: micro_operation & 0x3FFFFFFFFFFF, // cut of CRC
        }
    }

    /// Creates a new instruction from an opcode
    ///
    /// # Arguments
    ///
    /// * `opcode` - The opcode to create the instruction from
    ///
    /// # Returns
    ///
    /// Returns a new Instruction with only the opcode set
    pub const fn from_opcode(opcode: Opcode) -> Instruction {
        Self {
            instruction: (opcode as u16 as u64) << 32,
        }
    }

    /// Gets the raw opcode value from the instruction
    const fn opcode_raw(&self) -> u16 {
        ((self.instruction >> 32) & 0xFFF) as u16
    }

    /// Gets the opcode from the instruction
    ///
    /// # Returns
    ///
    /// Returns the Opcode enum value corresponding to the instruction's opcode
    pub fn opcode(&self) -> Opcode {
        Opcode::from_u16(self.opcode_raw())
            .expect("Since OpcodeEnum contains all possible values, this should never fail")
    }

    /// Assembles the instruction without calculating the CRC
    ///
    /// # Returns
    ///
    /// Returns the raw instruction value without CRC (like stored in the MSROM)
    pub fn assemble_no_crc(&self) -> u64 {
        self.instruction
    }

    /// Assembles the instruction with CRC
    ///
    /// # Returns
    ///
    /// Returns the complete instruction value including CRC (like stored in the MSRAM)
    pub fn assemble(&self) -> u64 {
        let instruction = self.assemble_no_crc();
        instruction | even_odd_parity_u64(instruction) << 46
    }

    /// Encodes an immediate value for source operand 1
    ///
    /// # Arguments
    ///
    /// * `src1_imm` - The immediate value to encode
    ///
    /// # Returns
    ///
    /// Returns the encoded immediate value
    const fn immediate_src1_encode(src1_imm: u64) -> u64 {
        (((src1_imm) & 0xff) << 24)
            | (((src1_imm) & 0x1f00) << 10)
            | (((src1_imm) & 0xe000) >> 7)
            | (1 << 9)
    }

    /// Encodes an instruction with a specifyable opcode
    ///
    /// # Arguments
    ///
    /// * `opcode` - The opcode to encode
    ///
    /// # Returns
    ///
    /// Returns the instructions without arguments
    const fn opcode_encode(opcode: Opcode) -> u64 {
        (opcode as u64) << 32
    }

    /// Creates an unconditional jump instruction
    ///
    /// # Arguments
    ///
    /// * `address` - The target address to jump to
    ///
    /// # Returns
    ///
    /// Returns a new Instruction representing an unconditional jump
    #[allow(non_snake_case)]
    pub fn UJMP<A: Into<UCInstructionAddress>>(address: A) -> Instruction {
        let value = Self::opcode_encode(Opcode::UJMP)
            | Self::immediate_src1_encode(address.into().address() as u64);
        Instruction::disassemble(value)
    }
}

impl Display for Instruction {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.instruction == 0 {
            write!(f, "NOP")
        } else {
            write!(f, "{}", self.opcode())
        }
    }
}

impl From<u64> for Instruction {
    fn from(instruction: u64) -> Self {
        Instruction::disassemble(instruction)
    }
}

// todo: add further disassembly/assembly methods

// todo: add tests
