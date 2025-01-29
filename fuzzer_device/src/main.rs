#![no_main]
#![no_std]

extern crate alloc;

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::{Display};
use data_types::addresses::UCInstructionAddress;
use fuzzer_device::cmos::NMIGuard;
use fuzzer_device::executor::{ExecutionResult, SampleExecutor};
use fuzzer_device::heuristic::{GlobalScores, Sample};
use fuzzer_device::mutation_engine::MutationEngine;
use iced_x86::{Decoder, DecoderOptions, Formatter, Instruction, NasmFormatter};
use log::error;
use num_traits::{ConstZero, SaturatingSub};
use rand_core::SeedableRng;
use uefi::{entry, print, println, Status};

#[entry]
unsafe fn main() -> Status {
    uefi::helpers::init().unwrap();
    println!("Hello world!");

    let _nmi_guard = NMIGuard::disable_nmi(true);

    let mut corpus = Vec::new();
    corpus.push(Sample::default());
    let mut executor = match SampleExecutor::new() {
        Ok(executor) => executor,
        Err(e) => {
            println!("Failed to create executor: {:?}", e);
            return Status::ABORTED;
        }
    };
    if !executor.selfcheck() {
        println!("Executor selfcheck failed");
        return Status::ABORTED;
    }
    let mutator = MutationEngine::default();
    let mut global_scores = GlobalScores::default();

    let mut random = random_source(0);

    // Declared global to avoid re-allocation
    let mut execution_result = ExecutionResult::default();

    // Coverage sofar
    let mut coverage_sofar = BTreeSet::<UCInstructionAddress>::new();

    // Global stats
    let mut global_stats = GlobalStats::default();

    // Execute initial sample to get ground truth coverage
    executor.execute_sample(&corpus[0], &mut execution_result);
    let ground_truth_coverage = execution_result.coverage.clone();

    // Main fuzzing loop
    loop {
        global_stats.iteration_count += 1;

        // Select a sample from the corpus
        let sample = global_scores
            .choose_sample_mut(&mut corpus, &mut random)
            .expect("There is at least one sample");

        // Mutate
        let (mutation, new_sample) = mutator.mutate_sample(sample, &mut random);

        println!("Running sample");
        disassemble_code(&new_sample.code_blob);

        // Execute
        executor.execute_sample(&new_sample, &mut execution_result);

        // Score
        let mut new_coverage = Vec::with_capacity(0);
        subtract_iter_btree(&mut execution_result.coverage, &ground_truth_coverage);
        for (address, count) in execution_result.coverage.iter() {
            if *count > 0 && !coverage_sofar.contains(address) {
                new_coverage.push(address);
            }
        }
        global_scores.update_best_recent_gain(new_coverage.len() as f64);
        let benefit = global_scores.benefit(new_coverage.len() as f64);

        // Update ratings
        sample
            .local_scores
            .update_ratings(benefit, mutation, &mut global_scores);

        // Add to corpus
        global_stats.iterations_since_last_gain += 1;
        if new_coverage.len() > 0 {
            global_stats.annonce_new_sample(&new_sample, new_coverage.len());
            global_stats.iterations_since_last_gain = 0;
            corpus.push(new_sample);
            coverage_sofar.extend(new_coverage);
            global_stats.coverage_sofar = coverage_sofar.len();
        }

        // Print status message
        global_stats.maybe_print();
    }

    Status::SUCCESS
}

fn subtract_iter_btree<'a, 'b, K: Ord + Display, V: SaturatingSub + Copy+ Display+ConstZero+Ord>(original: &'a mut BTreeMap<K, V>, subtract_this: &'b BTreeMap<K, V>) {
    original.retain(|k, v| {
    if let Some(subtract_v) = subtract_this.get(k) {
        if *v < *subtract_v {
            error!("Reducing coverage count for more than ground truth: {k} : {v} < {subtract_v}");
        }
        *v = v.saturating_sub(subtract_v);
    }
    !v.is_zero()
});
}

#[cfg(feature = "rand_isaac")]
pub fn random_source(seed: u64) -> rand_isaac::Isaac64Rng {
    rand_isaac::Isaac64Rng::seed_from_u64(seed)
}

#[derive(Debug, Default)]
pub struct GlobalStats {
    pub iteration_count: usize,
    pub iterations_since_last_gain: usize,
    pub coverage_sofar: usize,
}

impl GlobalStats {
    pub fn maybe_print(&self) {
        if self.iteration_count % 10000 == 0 || self.iterations_since_last_gain == 0 {
            self.print();
        }
    }
    pub fn print(&self) {
        println!("Iteration: {:e}", self.iteration_count);
        println!(" - since last: {:e}", self.iterations_since_last_gain);
        println!(" - coverage: {}", self.coverage_sofar);
    }

    pub fn annonce_new_sample(&self, sample: &Sample, new_coverage: usize) {
        println!("New sample added to corpus");
        println!("The sample increased coverage by: {}", new_coverage);
        disassemble_code(&sample.code_blob);
    }
}

fn disassemble_code(code: &[u8]) {
    let mut decoder = Decoder::with_ip(64, code, 0, DecoderOptions::NONE);
    let mut formatter = NasmFormatter::new();

    formatter.options_mut().set_digit_separator("`");
    formatter.options_mut().set_first_operand_char_index(10);

    let mut output = String::new();
    let mut instruction = Instruction::default();

    while decoder.can_decode() {
        decoder.decode_out(&mut instruction);
        output.clear();
        formatter.format(&instruction, &mut output);

        // Eg. "00007FFAC46ACDB2 488DAC2400FFFFFF     lea       rbp,[rsp-100h]"
        print!("{:016X} ", instruction.ip());
        let start_index = (instruction.ip() - 0) as usize;
        let instr_bytes = &code[start_index..start_index + instruction.len()];
        for b in instr_bytes.iter() {
            print!("{:02X}", b);
        }
        if instr_bytes.len() < 10 {
            for _ in 0..10 - instr_bytes.len() {
                print!("  ");
            }
        }
        println!(" {}", output);
    }
}