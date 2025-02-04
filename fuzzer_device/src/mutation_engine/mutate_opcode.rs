use alloc::boxed::Box;
use alloc::vec::Vec;
use iced_x86::OpKind;
use crate::heuristic::Sample;
use crate::mutation_engine::{InstructionDecoder, Mutation};
use log::{trace, warn};
use rand_core::RngCore;
use hypervisor::Page;
use crate::mutation_engine::random_mutation::RandomMutation;

pub struct MutateOperand {
    scratch_area: Box<Page>,
    decoder: InstructionDecoder,
    valid_instructions: Vec<usize>,
}

impl Default for MutateOperand {
    fn default() -> Self {
        Self {
            scratch_area: Page::alloc_zeroed(),
            decoder: InstructionDecoder::default(),
        }
    }
}

impl<R: RngCore> Mutation<R> for MutateOperand {
    fn mutate(&mut self, sample: &mut Sample, random: &mut R) {
        if sample.code_blob.is_empty() {
            warn!("Sample has no code blob, cannot mutate");
            return;
        }

        self.valid_instructions.clear();

        let decoded = self.decoder.decode(sample.code_blob.as_slice());

        for i in 0..decoded.len() {
            let decoded = decoded.get(i).expect("<= len");

            if !decoded.instruction.is_invalid() && decoded.instruction.op_count() > 0 {
                self.valid_instructions.push(i);
            }
        }

        if self.valid_instructions.is_empty() {
            #[cfg(feature = "__debug_print_mutation_info")]
            trace!("Mutate opcode: No valid instructions found in sample. Fallback to random mutation");
            return RandomMutation::default().mutate(sample, random);
        }

        let mut mutate_instruction_index = random.next_u32() as usize % self.valid_instructions.len();
        let instruction = decoded.get(self.valid_instructions[mutate_instruction_index]).expect("<= len").instruction.clone();

        let operand_to_mutate = random.next_u32() % instruction.op_count();
        let operand_kind = instruction.op_kind(operand_to_mutate);

        match operand_kind {
            OpKind::Register => {

            }
        }

        drop(decoded);
        let index = random.next_u32() as usize % sample.code_blob.len();
        sample.code_blob[index] = (random.next_u32() % u8::MAX as u32) as u8;
    }
}
