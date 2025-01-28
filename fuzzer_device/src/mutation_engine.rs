mod random_mutation;

use crate::heuristic::Sample;
use crate::mutation_engine::random_mutation::RandomMutation;
use alloc::boxed::Box;
use rand_core::RngCore;

pub const NUMBER_OF_MUTATION_OPERATIONS: usize = {
    if cfg!(feature = "mutation_random") {
        1
    } else {
        0
    }
};

const _: () = assert!(
    NUMBER_OF_MUTATION_OPERATIONS > 0,
    "At least one mutation operation must be enabled"
);

pub trait Mutation<R: RngCore> {
    fn mutate(&self, sample: &mut Sample, random: &mut R);
}

pub struct MutationEngine<R: RngCore> {
    mutations: [Box<dyn Mutation<R>>; NUMBER_OF_MUTATION_OPERATIONS],
}

impl<R: RngCore> Default for MutationEngine<R> {
    fn default() -> Self {
        Self {
            mutations: [Box::new(RandomMutation::default())],
        }
    }
}

impl<R: RngCore> MutationEngine<R> {
    pub fn mutate_sample(&self, sample: &Sample, random: &mut R) -> (usize, Sample) {
        let mut result = sample.clone();

        let mutation_index = sample.local_scores.choose_mutation(random);
        let mutation = self
            .mutations
            .get(mutation_index)
            .expect("Mutation index out of bounds");

        mutation.mutate(&mut result, random);

        (mutation_index, result)
    }
}
