#![no_std]

use data_types::addresses::{Address, UCInstructionAddress};
use data_types::patch::Triad;

pub mod dump;

pub struct RomDumpDissasembly<'a> {
    dissassembly: &'a str,
    model: u32,
}

impl<'a> RomDumpDissasembly<'a> {
    pub const fn new(dissassembly: &'a str, model: u32) -> Self {
        RomDumpDissasembly {
            dissassembly,
            model,
        }
    }

    pub fn dissassembly(&self) -> &'a str {
        self.dissassembly
    }

    pub fn model(&self) -> u32 {
        self.model
    }
}

pub struct RomDump<'a, 'b> {
    instructions: &'a [u64; 0x7c00],
    sequences: &'b [u32; 0x7c00 / 4],
    model: u32,
}

impl<'a, 'b> RomDump<'a, 'b> {
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

    pub fn instructions(&self) -> &'a [u64; 0x7c00] {
        self.instructions
    }

    pub fn sequence_words(&self) -> &'b [u32; 0x7c00 / 4] {
        self.sequences
    }

    pub fn get_instruction(&self, address: UCInstructionAddress) -> Option<u64> {
        self.instructions.get(address.address()).copied()
    }

    pub fn get_instruction_pair(&self, address: UCInstructionAddress) -> Option<[u64; 2]> {
        Some([
            self.get_instruction(address.align_even())?,
            self.get_instruction(address.align_even() + 1)?,
        ])
    }

    pub fn get_sequence_word(&self, address: UCInstructionAddress) -> Option<u32> {
        self.sequences.get(address.address() / 4).copied()
    }

    pub fn model(&self) -> u32 {
        self.model
    }

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
