use crate::mutation_engine::InstructionDecoder;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use iced_x86::IcedError;
use rand_core::RngCore;

const FENCE_INSTRUCTIONS: &[&[u8]] = &[
    // lfence
    &[0x0F, 0xAE, 0xE8],
    // mfence
    &[0x0F, 0xAE, 0xF0],
    // sfence
    &[0x0F, 0xAE, 0xF8],
    // nop
    &[0x90],
];

#[derive(Default)]
pub struct Serializer {
    instruction_decoder: InstructionDecoder,
}

impl Serializer {
    pub fn serialize_code<R: RngCore>(
        &mut self,
        random: &mut R,
        code: &[u8],
    ) -> Result<Vec<u8>, IcedError> {
        let decoded = self.instruction_decoder.decode(code);
        let mut output = Vec::with_capacity(decoded.len() * 4);

        for index in 0..decoded.len() {
            let instruction = decoded.get(index).expect("<= len");
            let fence_instruction =
                FENCE_INSTRUCTIONS[random.next_u32() as usize % FENCE_INSTRUCTIONS.len()];

            if instruction.instruction.is_invalid() {
                // just copy the bytes
                output.extend(instruction.bytes);
            } else {
                output.extend(instruction.bytes);
                output.extend(fence_instruction);
            }
        }

        Ok(output)
    }
}
