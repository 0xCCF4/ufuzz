//! Dynamic Microcode Compiler Library
//!
//! This library provides functionality for working with microcode instructions and sequence words.
//! It includes tools for assembling, disassembling, and manipulating microcode instructions
//! and sequence words.
//!
//! To this date, it is not a fully featured compiler, but implements the functionality
//! that is needed for the microcode instrumentation in other parts of the project.
//!
//! The main components are:
//! - [`instruction`]: Module for handling individual microcode instructions
//! - [`opcodes`]: Module containing all available microcode opcodes
//! - [`sequence_word`]: Module for handling sequence words
//! - [`Triad`]: A group of three instructions with an associated sequence word
#![no_std]

extern crate alloc;
extern crate core;

use crate::instruction::Instruction;
use crate::sequence_word::{DisassembleError, SequenceWord};
use core::fmt;
use core::fmt::{Display, Formatter};

pub mod instruction;
pub mod opcodes;
pub mod sequence_word;

impl Display for opcodes::Opcode {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// A group of three microcode instructions with an associated sequence word
///
/// A triad is the basic unit of microcode execution, containing three instructions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Triad {
    /// The three instructions in this triad
    pub instructions: [Instruction; 3],
    /// The sequence word controlling execution flow
    pub sequence_word: SequenceWord,
}

impl Triad {
    /// Assembles the triad into its binary representation
    ///
    /// # Returns
    ///
    /// Returns an array of four 64-bit values representing the assembled triad:
    /// - The first three values are the assembled instructions
    /// - The fourth value is the assembled sequence word (which is internally only a 32-bit value)
    pub fn assemble(&self) -> sequence_word::AssembleResult<[u64; 4]> {
        Ok([
            self.instructions[0].assemble(),
            self.instructions[1].assemble(),
            self.instructions[2].assemble(),
            self.sequence_word.assemble()? as u64,
        ])
    }
}

impl TryFrom<data_types::patch::Triad> for Triad {
    type Error = DisassembleError;

    /// Attempts to create a Triad from a patch triad
    ///
    /// # Arguments
    ///
    /// * `value` - The patch triad to convert
    ///
    /// # Returns
    ///
    /// Returns a Result containing the converted Triad if successful
    fn try_from(value: data_types::patch::Triad) -> Result<Self, Self::Error> {
        Ok(Triad {
            instructions: [
                Instruction::disassemble(value.instructions[0]),
                Instruction::disassemble(value.instructions[1]),
                Instruction::disassemble(value.instructions[2]),
            ],
            sequence_word: SequenceWord::disassemble_no_crc_check(value.sequence_word)?,
        })
    }
}

impl Display for Triad {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{} {} {} {}",
            self.instructions[0], self.instructions[1], self.instructions[2], self.sequence_word
        )
    }
}

/// Calculates even-odd parity for a 32-bit value
///
/// # Arguments
///
/// * `value` - The 32-bit value to calculate parity for
///
/// # Returns
///
/// Returns the calculated parity value
pub fn even_odd_parity_u32(mut value: u32) -> u32 {
    let mut result = 0;
    while value > 0 {
        result ^= value & 3;
        value >>= 2;
    }
    result
}

/// Calculates even-odd parity for a 64-bit value
///
/// # Arguments
///
/// * `value` - The 64-bit value to calculate parity for
///
/// # Returns
///
/// Returns the calculated parity value
pub fn even_odd_parity_u64(mut value: u64) -> u64 {
    let mut result = 0;
    while value > 0 {
        result ^= value & 3;
        value >>= 2;
    }
    result
}

/*
/// Mode for executing the next instruction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlFlowNextMode {
    /// Execution of next instruction ip+1
    Direct,
    /// Execution of next instruction SEQW GOTO
    Indirect,
}

/// Branch prediction state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlFlowBranchPrediction {
    /// Branch is predicted to be taken
    Taken,
    /// Branch is predicted to not be taken
    NotTaken,
    /// No prediction is made
    NotPredicted,
}

/// Control flow information for microcode execution
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlFlow {
    /// The next instruction is executed
    Next {
        /// Mode of next instruction execution
        mode: ControlFlowNextMode,
        /// Target address for execution
        target: UCInstructionAddress,
    },

    /// Conditional branch instruction
    ConditionalBranch {
        /// Branch prediction state
        prediction: ControlFlowBranchPrediction,
        /// Target address if branch is taken (if immediate)
        taken: Option<UCInstructionAddress>,
        /// Target address if branch is not taken
        not_taken: UCInstructionAddress,
    },

    /// Unconditional branch instruction
    UnconditionalBranch {
        /// Branch prediction state
        prediction: ControlFlowBranchPrediction,
        /// Next instruction address (if immediate)
        next: Option<UCInstructionAddress>,
    },

    /// Unknown control flow
    Unknown,
}

impl Opcode {
    /// Determines the control flow behavior of this opcode
    ///
    /// # Returns
    ///
    /// Returns the control flow information for this opcode
    pub fn control_flow(&self) -> ControlFlow {
        todo!() // todo, also see "Analyzing and Exploiting Branch Mispredictions in Microcode"
    }
}
*/
