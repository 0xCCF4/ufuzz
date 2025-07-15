//! Genetic algorithm implementation for fuzzing
//!
//! This module provides types and functionality for running a genetic algorithm
//! to evolve and optimize fuzzing samples.
//!
//! You should maybe consider to use AFL as mutator/input generator instead

use crate::instruction_corpus::CorpusInstruction;
use alloc::vec::Vec;
use core::cmp::Ordering;
use log::warn;
use rand::prelude::SliceRandom;
use rand_core::RngCore;
use serde::{Deserialize, Serialize};

/// Settings for the genetic algorithm pool
#[derive(Clone)]
pub struct GeneticPoolSettings {
    /// Size of the population to maintain
    pub population_size: usize,
    /// Size of each code sample in bytes
    pub code_size: usize,
    /// Number of random solutions to add each generation
    pub random_solutions_each_generation: usize,
    /// Number of best solutions to keep each generation
    pub keep_best_x_solutions: usize,
    /// Probability of random mutation (0.0 to 1.0)
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

/// A pool of samples for fuzzing
#[derive(Clone, Default)]
pub struct GeneticPool {
    /// Current population of samples
    population: Vec<Sample>,
    /// Settings for the genetic algorithm
    settings: GeneticPoolSettings,
}

impl GeneticPool {
    /// Create a new pool with random population
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

    /// Create a new pool with random population from an instruction corpus
    pub fn new_random_population_from_corpus<R: RngCore>(
        settings: GeneticPoolSettings,
        random: &mut R,
        corpus: &Vec<CorpusInstruction>,
    ) -> Self {
        let mut population = Vec::with_capacity(settings.population_size);
        for _ in 0..settings.population_size {
            let mut code = Vec::new();
            while code.len() < settings.code_size {
                let instruction = corpus
                    .get(random.next_u32() as usize % corpus.len())
                    .expect("should always be inbound");
                code.extend_from_slice(&instruction.bytes);
            }
            population.push(Sample::new(code));
        }
        Self {
            population,
            settings,
        }
    }

    /// Get all samples in the pool
    pub fn all_samples(&self) -> &[Sample] {
        &self.population
    }

    /// Get mutable access to all samples
    pub fn all_samples_mut(&mut self) -> &mut [Sample] {
        &mut self.population
    }

    /// Evolve the population by one generation
    pub fn evolution<R: RngCore>(&mut self, random: &mut R, fuzzing_feedback: bool) {
        if fuzzing_feedback {
            self.population.sort();
            self.population.reverse();
        } else {
            self.population.shuffle(random);
            warn!("Using random population shuffel!");
        }

        self.population
            .truncate(self.settings.keep_best_x_solutions);

        for _ in 0..self.settings.random_solutions_each_generation {
            self.population
                .push(Sample::random(self.settings.code_size, random));
        }

        let target_len = self.settings.population_size;
        while self.population.len() < target_len {
            let parent1 = &self.population[random.next_u32() as usize
                % (self.settings.keep_best_x_solutions
                    + self.settings.random_solutions_each_generation)];
            let parent2 = &self.population[random.next_u32() as usize
                % (self.settings.keep_best_x_solutions
                    + self.settings.random_solutions_each_generation)];
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

    /// Get the final sorted (by fitness) population
    pub fn result(mut self) -> Vec<Sample> {
        self.population.sort();
        self.population.reverse();
        self.population
    }
}

/// A single sample in the genetic pool
#[derive(Clone, PartialEq, Eq)]
pub struct Sample {
    /// Raw code bytes
    code_blob: Vec<u8>,
    /// Rating from execution (if available)
    pub rating: Option<GeneticSampleRating>,
}

impl Sample {
    /// Create a new sample with given code
    pub fn new(code_blob: Vec<u8>) -> Self {
        Self {
            code_blob,
            rating: None,
        }
    }

    /// Create a random sample of given size
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

    /// Get the code bytes
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

/// Rating for a genetic sample based on execution results
#[derive(Clone, PartialEq, Eq, Debug, Hash, Serialize, Deserialize, Default)]
pub struct GeneticSampleRating {
    /// Number of unique addresses covered
    pub unique_address_coverage: u16,
    /// Total number of address hits
    pub total_address_coverage: u32,
    /// Program utilization percentage (0-100)
    pub program_utilization: u8,
    /// Number of loops executed
    pub loop_count: u64,
}

impl GeneticSampleRating {
    /// Minimum possible rating
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
    use crate::genetic_pool::{GeneticPool, GeneticSampleRating, Sample};
    use alloc::vec;

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
