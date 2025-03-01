mod random_mutation;
pub mod serialize;

use crate::heuristic::Sample;
use crate::mutation_engine::random_mutation::RandomMutation;
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
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

            assert_eq!(
                mutations.len(),
                NUMBER_OF_MUTATION_OPERATIONS,
                "Number of mutations does not match the expected number"
            );
            mutations
        };

        Self { mutations }
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
