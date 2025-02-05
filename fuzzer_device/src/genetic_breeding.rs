use alloc::vec::Vec;
use log::warn;
use rand_core::RngCore;
use crate::executor::ExecutionResult;

type SampleRating = u64;


#[derive(Clone)]
pub struct GeneticPoolSettings {
    pub population_size: usize,
    pub code_size: usize,

    pub random_solutions_each_generation: usize,
    pub keep_best_x_solutions: usize,
    pub random_mutation_chance: f64,
}

impl Default for GeneticPoolSettings {
    fn default() -> Self {
        Self {
            population_size: 100,
            random_solutions_each_generation: 2,
            code_size: 10,
            keep_best_x_solutions: 10,
            random_mutation_chance: 0.01,
        }
    }
}

#[derive(Clone)]
pub struct GeneticPool {
    population: Vec<Sample>,
    settings: GeneticPoolSettings,

    scratch_pad: Vec<Sample>,
}

impl GeneticPool {
    pub fn new_random_population<R: RngCore>(settings: GeneticPoolSettings, random: &mut R) -> Self {
        let mut population = Vec::with_capacity(settings.population_size);
        for _ in 0..settings.population_size {
            population.push(Sample::random(settings.code_size, random));
        }
        Self { population, settings, scratch_pad: Vec::new() }
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
        let best_samples = &mut self.scratch_pad;
        best_samples.clear();

        best_samples.extend_from_slice(&self.population.as_slice()[0..self.settings.keep_best_x_solutions]);

        for _ in 0..self.settings.random_solutions_each_generation {
            best_samples.push(Sample::random(self.settings.code_size, random));
        }

        let target_len = self.population.len();
        self.population.clear();

        while self.population.len() < target_len {
            let parent1 = &best_samples[random.next_u32() as usize % best_samples.len()];
            let parent2 = &best_samples[random.next_u32() as usize % best_samples.len()];
            let mut child = parent1.clone();
            for j in (random.next_u32() as usize % self.settings.code_size)..self.settings.code_size {
                child.code_blob[j] = parent2.code_blob[j];
            }
            if (random.next_u32() as f64 / u32::MAX as f64) < self.settings.random_mutation_chance {
                let length = (random.next_u32() % 16).min(self.settings.code_size as u32) as usize;
                let offset = if self.settings.code_size == length { 0 } else { random.next_u32() as usize % (self.settings.code_size - length) };
                for i in 0..length {
                    child.code_blob[i+offset] = random.next_u32() as u8;
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
    code_blob: Vec<u8>,
    rating: Option<SampleRating>,
}

impl Sample {
    pub fn random<R: RngCore>(code_size: usize, random: &mut R) -> Self {
        let mut code_blob = Vec::with_capacity(code_size);
        for _ in 0..code_size {
            code_blob.push((random.next_u32() % u8::MAX as u32) as u8);
        }
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