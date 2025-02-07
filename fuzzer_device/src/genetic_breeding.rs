use crate::executor::ExecutionResult;
use crate::mutation_engine::InstructionDecoder;
use alloc::vec::Vec;
use iced_x86::FlowControl;
use log::warn;
use rand_core::RngCore;

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
}

impl GeneticPool {
    pub fn new_random_population<R: RngCore>(
        settings: GeneticPoolSettings,
        random: &mut R,
    ) -> Self {
        let mut population = Vec::with_capacity(settings.population_size);
        for _ in 0..settings.population_size {
            population.push(Sample::random(settings.code_size, random));
        }
        Self {
            population,
            settings,
        }
    }
    pub fn all_samples(&self) -> &[Sample] {
        &self.population
    }
    pub fn all_samples_mut(&mut self) -> &mut [Sample] {
        &mut self.population
    }
    pub fn evolution<R: RngCore>(&mut self, random: &mut R) {
        self.population.sort_by_key(|s| {
            s.rating.unwrap_or_else(|| {
                warn!("Sample not rated!");
                0
            })
        });
        self.population.reverse();

        self.population
            .truncate(self.settings.keep_best_x_solutions);

        for _ in 0..self.settings.random_solutions_each_generation {
            self.population
                .push(Sample::random(self.settings.code_size, random));
        }

        let target_len = self.settings.population_size;
        while self.population.len() < target_len {
            let parent1 = &self.population[random.next_u32() as usize % self.population.len()];
            let parent2 = &self.population[random.next_u32() as usize % self.population.len()];
            let mut child = parent1.clone();
            for j in (random.next_u32() as usize % self.settings.code_size)..self.settings.code_size
            {
                child.code_blob[j] = parent2.code_blob[j];
            }
            if (random.next_u32() as f64 / u32::MAX as f64) < self.settings.random_mutation_chance {
                let length = (random.next_u32() % 16).min(self.settings.code_size as u32) as usize;
                let offset = if self.settings.code_size == length {
                    0
                } else {
                    random.next_u32() as usize % (self.settings.code_size - length)
                };
                for i in 0..length {
                    child.code_blob[i + offset] = random.next_u32() as u8;
                }
            }
            self.population.push(child);
        }
    }
    pub fn result(mut self) -> Vec<Sample> {
        self.population.sort_by_key(|s| {
            s.rating.unwrap_or_else(|| {
                warn!("Sample not rated!");
                0
            })
        });
        self.population.reverse();
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
        Self {
            code_blob,
            rating: None,
        }
    }
    pub fn rate(&mut self, rating: SampleRating) {
        self.rating = Some(rating);
    }
    pub fn rating(&self) -> Option<SampleRating> {
        self.rating
    }
    pub fn code(&self) -> &[u8] {
        &self.code_blob
    }
    pub fn unrate(&mut self) {
        self.rating = None;
    }

    pub fn rate_from_execution(
        &mut self,
        execution_result: &ExecutionResult,
        decoder: &mut InstructionDecoder,
    ) {
        let decoded = decoder.decode(self.code_blob.as_slice());
        let mut instructions = execution_result.trace.hit.keys().into_iter()
            .filter_map(|&ip| decoded.instruction_by_ip(ip as usize));

        let unique_trace_points = execution_result.trace.hit.keys().len();

        let multiplier = if instructions.any(|i| i.instruction.flow_control() != FlowControl::IndirectBranch)
        {
            0.1
        } else {
            1.0
        };

        drop(decoded);

        self.rate((unique_trace_points as f32 * 100.0 * multiplier as f32) as SampleRating);
    }
}
