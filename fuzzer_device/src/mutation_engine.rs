mod random_mutation;

use crate::heuristic::Sample;
use crate::mutation_engine::random_mutation::RandomMutation;
use alloc::boxed::Box;
use alloc::vec::Vec;
use iced_x86::{Decoder, DecoderOptions};
use rand_core::RngCore;

pub const NUMBER_OF_MUTATION_OPERATIONS: usize = {
    const MUT_RANDOM: usize = if cfg!(feature = "mutation_random") {
        1
    } else {
        0
    };

    MUT_RANDOM
};

const _: () = assert!(
    NUMBER_OF_MUTATION_OPERATIONS > 0,
    "At least one mutation operation must be enabled"
);

pub trait Mutation<R: RngCore> {
    fn mutate(&mut self, sample: &mut Sample, random: &mut R);
}

pub struct MutationEngine<R: RngCore> {
    mutations: Vec<Box<dyn Mutation<R>>>,
}

impl<R: RngCore> Default for MutationEngine<R> {
    fn default() -> Self {
        let mutations = {
            let mut mutations: Vec<Box<dyn Mutation<R>>> = Vec::default();

            if cfg!(feature = "mutation_random") {
                mutations.push(Box::new(RandomMutation::default()));
            }

            assert_eq!(mutations.len(), NUMBER_OF_MUTATION_OPERATIONS, "Number of mutations does not match the expected number");
            mutations
        };

        Self {
            mutations
        }
    }
}

impl<R: RngCore> MutationEngine<R> {
    pub fn mutate_sample(&mut self, sample: &Sample, random: &mut R) -> (usize, Sample) {
        let mut result = sample.clone();

        let mutation_index = sample.local_scores.choose_mutation(random);
        let mutation: &mut Box<dyn Mutation<R>> = self
            .mutations
            .get_mut(mutation_index)
            .expect("Mutation index out of bounds");

        mutation.mutate(&mut result, random);

        (mutation_index, result)
    }
}

pub struct InstructionWithBytes<'a> {
    pub instruction: iced_x86::Instruction,
    pub bytes: &'a [u8],
}

#[derive(Default)]
pub struct InstructionDecoder {
    buffer: Vec<InstructionWithBytes<'static>>,
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
}

impl Drop for InstructionDecodeResult<'_> {
    fn drop(&mut self) {
        self.decoder.buffer.clear();
    }
}

impl InstructionDecoder {
    pub fn new() -> Self {
        Self {
            buffer: Vec::default()
        }
    }

    /// Safety: safe, inputs must stay valid until output is dropped, which will invalidate the unsafe references created
    pub fn decode<'output, 'this: 'output, 'instructions: 'output>(&'this mut self, instructions: &'instructions [u8]) -> InstructionDecodeResult<'output> {
        let mut decoder = Decoder::with_ip(64, instructions, 0, DecoderOptions::NONE);
        
        let mut instruction_start_index = 0;
        
        while decoder.can_decode() {
            let instruction = decoder.decode();
            let instruction_end_index = instruction_start_index + instruction.len();

            let instruction_data: &'instructions [u8] = &instructions[instruction_start_index..instruction_end_index];
            let instruction_data_static: &'static [u8] = unsafe { core::mem::transmute(instruction_data) };

            self.buffer.push(InstructionWithBytes {
                instruction,
                bytes: instruction_data_static,
            });

            instruction_start_index = instruction_end_index;
        }

        InstructionDecodeResult {
            decoder: self,
        }
    }
}