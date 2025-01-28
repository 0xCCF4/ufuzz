use crate::mutation_engine::NUMBER_OF_MUTATION_OPERATIONS;
use alloc::vec;
use alloc::vec::Vec;
use rand_core::RngCore;

const NUMBER_OF_MUTATION_OPERATIONS_INV: f64 = 1.0 / NUMBER_OF_MUTATION_OPERATIONS as f64;

const LOCAL_ALPHA: f64 = 0.9;
const LOCAL_A: f64 = 10.0;
const LOCAL_THETA: f64 = 0.9;
const GLOBAL_THETA: f64 = 0.75;
const GLOBAL_GAMMA: f64 = 0.01;
const GLOBAL_BETA: f64 = 0.1;
const GLOBAL_EPSILON: f64 = 0.99;

fn sigmoid(x: f64) -> f64 {
    1.0 / (1.0 + libm::exp((-x + 0.5) * 10.0))
}

#[derive(Clone)]
pub struct Sample {
    pub code_blob: Vec<u8>,

    pub local_scores: LocalScores,
}

#[derive(Clone)]
pub struct LocalScores {
    pub local_usability: [f64; NUMBER_OF_MUTATION_OPERATIONS],
    cache_rating: Option<f64>,
}

pub struct GlobalScores {
    pub global_usability: [f64; NUMBER_OF_MUTATION_OPERATIONS],
    pub best_recent_gain: f64,
}

impl LocalScores {
    fn calculate_rating(&self) -> f64 {
        let mut cache_rating = 0.0;
        for i in 0..NUMBER_OF_MUTATION_OPERATIONS {
            cache_rating += libm::pow(
                self.local_usability[i] - NUMBER_OF_MUTATION_OPERATIONS_INV,
                2.0,
            );
        }
        cache_rating
    }
    pub fn get_rating(&self) -> f64 {
        self.cache_rating.unwrap_or_else(|| self.calculate_rating())
    }
    pub fn update_ratings(
        &mut self,
        benefit: f64,
        mutation_chosen: usize,
        global_usability: &mut GlobalScores,
    ) {
        let old = self.local_usability[mutation_chosen];
        let global = global_usability.global_usability[mutation_chosen];

        let new = sigmoid(LOCAL_ALPHA * old + (1.0 - LOCAL_ALPHA) * global + LOCAL_A * benefit);

        self.local_usability[mutation_chosen] = new.clamp(0.0, 1.0);

        let factor = (new - old) * NUMBER_OF_MUTATION_OPERATIONS_INV;

        for i in 0..NUMBER_OF_MUTATION_OPERATIONS {
            if i == mutation_chosen {
                continue;
            }
            self.local_usability[i] = (self.local_usability[i] + factor).clamp(0.0, 1.0);
        }

        self.cache_rating = Some(self.calculate_rating());

        global_usability.update_ratings(mutation_chosen, self);
    }
    pub fn choose_mutation<R: RngCore>(&self, random: &mut R) -> usize {
        let p = random.next_u64() as f64 / u64::MAX as f64;
        if p < LOCAL_THETA {
            // choose best mutation
            let mut best_sofar = None;
            for i in 0..NUMBER_OF_MUTATION_OPERATIONS {
                match best_sofar {
                    None => best_sofar = Some((i, self.local_usability[i])),
                    Some((_, best_score_sofar)) => {
                        let current_score = self.local_usability[i];
                        if current_score < best_score_sofar {
                            best_sofar = Some((i, current_score));
                        }
                    }
                }
            }
            best_sofar.expect("There should be at least one mutation").0
        } else {
            // choose random mutation
            random.next_u32() as usize % NUMBER_OF_MUTATION_OPERATIONS
        }
    }
}

impl GlobalScores {
    pub fn choose_sample<'a, R: RngCore>(
        &self,
        samples: &'a [Sample],
        random: &mut R,
    ) -> Option<&'a Sample> {
        let p = random.next_u64() as f64 / u64::MAX as f64;
        if p < GLOBAL_THETA {
            // choose the best sample
            let mut best_sofar = None;
            for sample in samples {
                match best_sofar {
                    None => best_sofar = Some((sample.local_scores.get_rating(), sample)),
                    Some((best_score_sofar, _)) => {
                        let current_score = sample.local_scores.get_rating();
                        if current_score < best_score_sofar {
                            best_sofar = Some((current_score, sample));
                        }
                    }
                }
            }
            match best_sofar {
                None => None,
                Some((_, best_sample)) => Some(best_sample),
            }
        } else {
            // choose a random sample
            let random_index = random.next_u32() as usize % samples.len();
            if samples.len() > 0 {
                Some(&samples[random_index])
            } else {
                None
            }
        }
    }
    pub fn choose_sample_mut<'a, R: RngCore>(
        &self,
        samples: &'a mut [Sample],
        random: &mut R,
    ) -> Option<&'a mut Sample> {
        let p = random.next_u64() as f64 / u64::MAX as f64;
        if p < LOCAL_THETA {
            // choose the best sample
            let mut best_sofar = None;
            for sample in samples {
                match best_sofar {
                    None => best_sofar = Some((sample.local_scores.get_rating(), sample)),
                    Some((best_score_sofar, _)) => {
                        let current_score = sample.local_scores.get_rating();
                        if current_score < best_score_sofar {
                            best_sofar = Some((current_score, sample));
                        }
                    }
                }
            }
            match best_sofar {
                None => None,
                Some((_, best_sample)) => Some(best_sample),
            }
        } else {
            // choose a random sample
            let random_index = random.next_u32() as usize % samples.len();
            if samples.len() > 0 {
                Some(&mut samples[random_index])
            } else {
                None
            }
        }
    }
    pub fn update_ratings(&mut self, mutation_chosen: usize, local_usability: &LocalScores) {
        let old = self.global_usability[mutation_chosen];
        let local = local_usability.local_usability[mutation_chosen];

        let new = sigmoid(
            GLOBAL_GAMMA * NUMBER_OF_MUTATION_OPERATIONS_INV
                + (1.0 - GLOBAL_GAMMA) * sigmoid(GLOBAL_BETA * local + (1.0 - GLOBAL_BETA) * old),
        );

        self.global_usability[mutation_chosen] = new.clamp(0.0, 1.0);

        let factor = (new - old) * NUMBER_OF_MUTATION_OPERATIONS_INV;

        for i in 0..NUMBER_OF_MUTATION_OPERATIONS {
            if i == mutation_chosen {
                continue;
            }
            self.global_usability[i] = (self.global_usability[i] + factor).clamp(0.0, 1.0);
        }
    }
    pub fn update_best_recent_gain(&mut self, gain: f64) {
        self.best_recent_gain = GLOBAL_EPSILON * libm::fmax(self.best_recent_gain, gain);
    }
    pub fn benefit(&self, gain: f64) -> f64 {
        gain / self.best_recent_gain
    }
}

impl Default for LocalScores {
    fn default() -> Self {
        Self {
            cache_rating: Some(0.0),
            local_usability: [NUMBER_OF_MUTATION_OPERATIONS_INV; NUMBER_OF_MUTATION_OPERATIONS],
        }
    }
}

impl Default for GlobalScores {
    fn default() -> Self {
        Self {
            global_usability: [NUMBER_OF_MUTATION_OPERATIONS_INV; NUMBER_OF_MUTATION_OPERATIONS],
            best_recent_gain: 0.0,
        }
    }
}

impl Default for Sample {
    fn default() -> Self {
        Self {
            code_blob: vec![0x90 /* NOP */],
            local_scores: LocalScores::default(),
        }
    }
}
