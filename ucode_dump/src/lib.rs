//! # Microcode dumps
//!
//! A library for handling and analyzing CPU microcode ROM dumps.
//! This crate provides functionality for working with microcode dumps and includes dumps of known architectures.
#![no_std]

use data_types::addresses::{Address, UCInstructionAddress};
use data_types::patch::Triad;

pub mod dump;

/// Represents a disassembled ROM dump with model information
pub struct RomDumpDissasembly<'a> {
    dissassembly: &'a str,
    model: u32,
}

impl<'a> RomDumpDissasembly<'a> {
    /// Creates a new ROM dump disassembly instance
    ///
    /// # Arguments
    /// * `dissassembly` - The disassembled microcode text
    /// * `model` - The CPU model identifier
    pub const fn new(dissassembly: &'a str, model: u32) -> Self {
        RomDumpDissasembly {
            dissassembly,
            model,
        }
    }

    /// Returns the disassembled microcode text
    pub fn dissassembly(&self) -> &'a str {
        self.dissassembly
    }

    /// Returns the CPU model identifier
    pub fn model(&self) -> u32 {
        self.model
    }
}

/// Represents a microcode ROM dump with instructions and sequences
pub struct RomDump<'a, 'b> {
    instructions: &'a [u64; 0x7c00],
    sequences: &'b [u32; 0x7c00 / 4],
    model: u32,
}

impl<'a, 'b> RomDump<'a, 'b> {
    /// Creates a new ROM dump instance
    ///
    /// # Arguments
    /// * `instructions` - Array of microcode instructions
    /// * `sequences` - Array of sequence words
    /// * `model` - CPU model identifier
    pub const fn new(
        instructions: &'a [u64; 0x7c00],
        sequences: &'b [u32; 0x7c00 / 4],
        model: u32,
    ) -> Self {
        RomDump {
            instructions,
            sequences,
            model,
        }
    }

    /// Returns the array of microcode instructions
    pub fn instructions(&self) -> &'a [u64; 0x7c00] {
        self.instructions
    }

    /// Returns the array of sequence words
    pub fn sequence_words(&self) -> &'b [u32; 0x7c00 / 4] {
        self.sequences
    }

    /// Retrieves a single instruction at the specified address
    ///
    /// # Arguments
    /// * `address` - The instruction address
    ///
    /// # Returns
    /// * `Option<u64>` - The instruction if found, None otherwise
    pub fn get_instruction(&self, address: UCInstructionAddress) -> Option<u64> {
        self.instructions.get(address.address()).copied()
    }

    /// Retrieves a pair of instructions starting at the specified address
    ///
    /// # Arguments
    /// * `address` - The starting instruction address
    ///
    /// # Returns
    /// * `Option<[u64; 2]>` - The instruction pair if found, None otherwise
    pub fn get_instruction_pair(&self, address: UCInstructionAddress) -> Option<[u64; 2]> {
        Some([
            self.get_instruction(address.align_even())?,
            self.get_instruction(address.align_even() + 1)?,
        ])
    }

    /// Retrieves a sequence word at the specified address
    ///
    /// # Arguments
    /// * `address` - The instruction address
    ///
    /// # Returns
    /// * `Option<u32>` - The sequence word if found, None otherwise
    pub fn get_sequence_word(&self, address: UCInstructionAddress) -> Option<u32> {
        self.sequences.get(address.address() / 4).copied()
    }

    /// Returns the CPU model identifier
    pub fn model(&self) -> u32 {
        self.model
    }

    /// Retrieves a triad (3 instructions and sequence word) at the specified address
    ///
    /// # Arguments
    /// * `address` - The instruction address
    ///
    /// # Returns
    /// * `Option<Triad>` - The triad if found, None otherwise
    pub fn triad(&self, address: UCInstructionAddress) -> Option<Triad> {
        let base = address.triad_base();
        let triad = Triad {
            instructions: [
                self.get_instruction(base)?,
                self.get_instruction(base + 1)?,
                self.get_instruction(base + 2)?,
            ],
            sequence_word: self.get_sequence_word(base)?,
        };
        Some(triad)
    }
}
