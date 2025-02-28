use alloc::vec::Vec;
use core::cmp::Ordering;
use rand_core::RngCore;
use serde::{Deserialize, Serialize};

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
            code_size: 32,
            keep_best_x_solutions: 10,
            random_mutation_chance: 0.01,
        }
    }
}

#[derive(Clone, Default)]
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
        self.population.sort();
        self.population.reverse();

        self.population
            .truncate(self.settings.keep_best_x_solutions);

        for _ in 0..self.settings.random_solutions_each_generation {
            self.population
                .push(Sample::random(self.settings.code_size, random));
        }

        let target_len = self.settings.population_size;
        while self.population.len() < target_len {
            let parent1 = &self.population[random.next_u32() as usize % (self.settings.keep_best_x_solutions+self.settings.random_solutions_each_generation)];
            let parent2 = &self.population[random.next_u32() as usize % (self.settings.keep_best_x_solutions+self.settings.random_solutions_each_generation)];
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
        self.population.sort();
        self.population.reverse();
        self.population
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Sample {
    code_blob: Vec<u8>,
    pub rating: Option<GeneticSampleRating>,
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
    pub fn code(&self) -> &[u8] {
        &self.code_blob
    }
}

impl Ord for Sample {
    fn cmp(&self, other: &Self) -> Ordering {
        self.rating.cmp(&other.rating)
    }
}

impl PartialOrd for Sample {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.rating.partial_cmp(&other.rating)
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Hash, Serialize, Deserialize, Default)]
pub struct GeneticSampleRating {
    pub unique_address_coverage: u16,
    pub total_address_coverage: u32,
    pub program_utilization: u8, // in range 0 = 0% to 100 = 100%
    pub loop_count: u64,
}

impl GeneticSampleRating {
    pub const MIN: Self = Self {
        unique_address_coverage: 0,
        total_address_coverage: 0,
        program_utilization: 0,
        loop_count: 0,
    };
}

impl Ord for GeneticSampleRating {
    fn cmp(&self, other: &Self) -> Ordering {
        let unique_address_coverage_order = self
            .unique_address_coverage
            .cmp(&other.unique_address_coverage);

        if unique_address_coverage_order != Ordering::Equal {
            return unique_address_coverage_order;
        }

        let total_address_coverage_order = self
            .total_address_coverage
            .cmp(&other.total_address_coverage);

        if total_address_coverage_order != Ordering::Equal {
            return total_address_coverage_order;
        }

        let program_utilization_order = self
            .program_utilization
            .partial_cmp(&other.program_utilization)
            .unwrap_or(Ordering::Equal);

        if program_utilization_order != Ordering::Equal {
            return program_utilization_order;
        }

        self.loop_count.cmp(&other.loop_count)
    }
}

impl PartialOrd for GeneticSampleRating {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.cmp(other).into()
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec;
    use crate::genetic_pool::{GeneticPool, GeneticSampleRating, Sample};

    #[test]
    pub fn test_genetic_pool() {
        let sample1 = Sample {
            rating: Some(GeneticSampleRating {
                unique_address_coverage: 10,
                total_address_coverage: 100,
                program_utilization: 50,
                loop_count: 5,
            }),
            code_blob: vec![1],
        };
        let sample2 = Sample {
            rating: Some(GeneticSampleRating {
                unique_address_coverage: 10,
                total_address_coverage: 110,
                program_utilization: 50,
                loop_count: 5,
            }),
            code_blob: vec![2],
        };
        let mut pool = GeneticPool {
            settings: Default::default(),
            population: vec![sample1, sample2],
        };
        pool.population.sort();
        pool.population.reverse();
        assert_eq!(pool.population[0].code_blob, vec![2]);

    }
}
