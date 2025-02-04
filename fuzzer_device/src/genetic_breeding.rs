use alloc::vec::Vec;
use log::warn;
use rand_core::RngCore;
use uefi::println;
use crate::executor::ExecutionResult;

const CODE_SIZE: usize = 10;
type SampleRating = u64;

const SELECT_BEST_X_PERCENT: f64 = 0.1;
const RANDOM_MUTATION_CHANCE: f64 = 0.001;
const RANDOM_SELECTION_PERCENTAGE: f64 = 0.01;

#[derive(Clone)]
pub struct GeneticPool {
    population: Vec<Sample>,
}

impl GeneticPool {
    pub fn new_random_population<R: RngCore>(size: usize, random: &mut R) -> Self {
        let mut population = Vec::with_capacity(size);
        for _ in 0..size {
            population.push(Sample::random(random));
        }
        Self { population }
    }
    pub fn all_samples(&self) -> &[Sample] {
        &self.population
    }
    pub fn all_samples_mut(&mut self) -> &mut [Sample] {
        &mut self.population
    }
    pub fn evolution<R: RngCore>(&mut self, random: &mut R) {
        self.population.sort_by_key(|s| s.rating.unwrap_or_else(|| {
            warn!("Sample not rated!");
            0
        }));
        let best_samples = self.population.as_slice()[0..(self.population.len() as f64 * SELECT_BEST_X_PERCENT) as usize].to_vec();

        for _ in 0..(self.population.len() as f64 * RANDOM_SELECTION_PERCENTAGE) as usize {
            self.population.push(Sample::random(random));
        }

        let target_len = self.population.len();
        self.population.clear();

        while self.population.len() < target_len {
            let parent1 = &best_samples[random.next_u32() as usize % best_samples.len()];
            let parent2 = &best_samples[random.next_u32() as usize % best_samples.len()];
            let mut child = parent1.clone();
            for i in 0..CODE_SIZE {
                for j in (random.next_u32() as usize % CODE_SIZE)..CODE_SIZE {
                    child.code_blob[j] = parent2.code_blob[j];
                }
                if (random.next_u32() as f64 / u32::MAX as f64) < RANDOM_MUTATION_CHANCE {
                    child.code_blob[i] = random.next_u32() as u8;
                }
            }
            self.population.push(child);
        }
    }
    pub fn result(mut self) -> Vec<Sample> {
        self.population.sort_by_key(|s| s.rating.unwrap_or_else(|| {
            warn!("Sample not rated!");
            0
        }));
        self.population
    }
}

#[derive(Clone)]
pub struct Sample {
    code_blob: [u8; CODE_SIZE],
    rating: Option<SampleRating>,
}

impl Sample {
    pub fn random<R: RngCore>(random: &mut R) -> Self {
        let mut code_blob = [0; CODE_SIZE];
        random.fill_bytes(&mut code_blob);
        Self { code_blob, rating: None }
    }
    pub fn rate(&mut self, rating: SampleRating) {
        self.rating = Some(rating);
    }
    pub fn code(&self) -> &[u8] {
        &self.code_blob
    }
    pub fn unrate(&mut self) {
        self.rating = None;
    }

    pub fn rate_from_execution(&mut self, execution_result: &ExecutionResult) {
        self.rate(execution_result.trace.len() as SampleRating);
    }
}