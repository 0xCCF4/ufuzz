use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use iced_x86::{Decoder, DecoderOptions};

pub struct InstructionWithBytes<'a> {
    pub instruction: iced_x86::Instruction,
    pub bytes: &'a [u8],
}

#[derive(Default)]
pub struct InstructionDecoder {
    buffer: Vec<InstructionWithBytes<'static>>,
    instruction_map: BTreeMap<usize, usize>,
}

impl Clone for InstructionDecoder {
    fn clone(&self) -> Self {
        InstructionDecoder::default()
    }
}

pub struct InstructionDecodeResult<'a> {
    decoder: &'a mut InstructionDecoder,
}

impl<'a> InstructionDecodeResult<'a> {
    pub fn len(&self) -> usize {
        self.decoder.buffer.len()
    }
    pub fn get(&'a self, index: usize) -> Option<&'a InstructionWithBytes<'a>> {
        self.decoder.buffer.get(index)
    }
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
    pub fn new() -> Self {
        Self {
            buffer: Vec::default(),
            instruction_map: BTreeMap::default(),
        }
    }

    /// Safety: safe, inputs must stay valid until output is dropped, which will invalidate the unsafe references created
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
