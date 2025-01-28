use crate::heuristic::Sample;
use crate::mutation_engine::Mutation;
use log::warn;
use rand_core::RngCore;

#[derive(Default)]
pub struct RandomMutation;

impl<R: RngCore> Mutation<R> for RandomMutation {
    fn mutate(&self, sample: &mut Sample, random: &mut R) {
        if sample.code_blob.is_empty() {
            warn!("Sample has no code blob, cannot mutate");
            return;
        }

        let index = random.next_u32() as usize % sample.code_blob.len();
        sample.code_blob[index] = (random.next_u32() % u8::MAX as u32) as u8;
    }
}
