//! x86 instruction decoding
//!
//! This module provides functionality for decoding x86 instructions using the
//! iced-x86 decoder. It maintains a mapping between instruction addresses and
//! their decoded forms. This allows mapping bytes to decoded instruction stream.
//! Further, the same memory allocation is reused accross different calls to the decoder.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use iced_x86::{Decoder, DecoderOptions};

/// A decoded instruction with its raw bytes
pub struct InstructionWithBytes<'a> {
    /// The decoded instruction
    pub instruction: iced_x86::Instruction,
    /// Raw bytes of the instruction
    pub bytes: &'a [u8],
}

/// State for decoding instructions
#[derive(Default)]
pub struct InstructionDecoder {
    /// Buffer of decoded instructions
    buffer: Vec<InstructionWithBytes<'static>>,
    /// Map from instruction address to buffer index
    instruction_map: BTreeMap<usize, usize>,
}

impl Clone for InstructionDecoder {
    fn clone(&self) -> Self {
        InstructionDecoder::default()
    }
}

/// Result of decoding a sequence of instructions
pub struct InstructionDecodeResult<'a> {
    /// Reference to the decoder that produced this result
    decoder: &'a mut InstructionDecoder,
}

impl<'a> InstructionDecodeResult<'a> {
    /// Get the number of decoded instructions
    pub fn len(&self) -> usize {
        self.decoder.buffer.len()
    }

    /// Get a decoded instruction by index
    pub fn get(&'a self, index: usize) -> Option<&'a InstructionWithBytes<'a>> {
        self.decoder.buffer.get(index)
    }

    /// Get a decoded instruction by instruction pointer
    pub fn instruction_by_ip(&'a self, ip: usize) -> Option<&'a InstructionWithBytes<'a>> {
        self.decoder
            .instruction_map
            .get(&ip)
            .map(|index| self.get(*index).expect("index is valid"))
    }
}

impl Drop for InstructionDecodeResult<'_> {
    fn drop(&mut self) {
        self.decoder.buffer.clear();
        self.decoder.instruction_map.clear();
    }
}

impl InstructionDecoder {
    /// Create a new instruction decoder
    pub fn new() -> Self {
        Self {
            buffer: Vec::default(),
            instruction_map: BTreeMap::default(),
        }
    }

    /// Decode a sequence of instructions
    pub fn decode<'output, 'this: 'output, 'instructions: 'output>(
        &'this mut self,
        instructions: &'instructions [u8],
    ) -> InstructionDecodeResult<'output> {
        let mut decoder = Decoder::with_ip(64, instructions, 0, DecoderOptions::NONE);

        let mut instruction_start_index = 0;

        while decoder.can_decode() {
            let instruction = decoder.decode();
            let instruction_end_index = instruction_start_index + instruction.len();

            let instruction_data: &'instructions [u8] =
                &instructions[instruction_start_index..instruction_end_index];
            let instruction_data_static: &'static [u8] =
                unsafe { core::mem::transmute(instruction_data) };

            self.buffer.push(InstructionWithBytes {
                instruction,
                bytes: instruction_data_static,
            });

            for i in instruction_start_index..instruction_end_index {
                self.instruction_map.insert(i, self.buffer.len() - 1);
            }

            instruction_start_index = instruction_end_index;
        }

        InstructionDecodeResult { decoder: self }
    }
}
