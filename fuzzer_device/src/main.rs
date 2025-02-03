#![allow(incomplete_features)]
#![feature(generic_const_exprs)]
#![no_main]
#![no_std]

extern crate alloc;

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Display;
use core::ops::DerefMut;
use data_types::addresses::UCInstructionAddress;
use fuzzer_device::cmos::CMOS;
use fuzzer_device::executor::{ExecutionEvent, ExecutionResult, SampleExecutor};
use fuzzer_device::heuristic::{GlobalScores, Sample};
use fuzzer_device::mutation_engine::MutationEngine;
use fuzzer_device::{disassemble_code, PersistentApplicationData, PersistentApplicationState};
use hypervisor::state::StateDifference;
use log::{debug, error, info, trace, warn};
use num_traits::{ConstZero, SaturatingSub};
use rand_core::SeedableRng;
use uefi::proto::loaded_image::LoadedImage;
use uefi::proto::media::file::{File, FileAttribute, FileMode};
use uefi::{entry, println, CString16, Error, Status};

#[entry]
unsafe fn main() -> Status {
    uefi::helpers::init().unwrap();
    println!("Hello world!");
    println!(
        "Program version: {:08x}",
        PersistentApplicationData::this_app_version()
    );

    prepare_gdb();

    let disable_nmi = true;
    let mut cmos = CMOS::<PersistentApplicationData>::read_from_ram(disable_nmi);
    let mut excluded_addresses = match ExcludedAddresses::load_file() {
        Ok(excluded_addresses) => excluded_addresses,
        Err(e) => {
            error!("Failed to load excluded addresses: {:?}", e);
            return Status::ABORTED;
        }
    };
    // Read application data from last run from cmos
    if cmos.is_data_valid() {
        info!("CMOS checksum is valid");
    } else {
        info!("CMOS checksum is invalid.");
    }
    let mut cmos_data = cmos.data_mut_or_insert();
    if !cmos_data.is_same_program_version() {
        // if app data was from previous program version, reset
        trace!("Program version changed. Erasing CMOS data.");
        *cmos_data.deref_mut() = PersistentApplicationData::default();
    }
    if let PersistentApplicationState::CollectingCoverage(address) = cmos_data.state {
        // failed to collect coverage from that address
        excluded_addresses.exclude_address(address);
        info!(
            "Last run failed to collect coverage from address: {:x?}",
            address
        );
        if let Err(err) = excluded_addresses.save_file() {
            error!("Failed to save excluded addresses: {:?}", err);
            return Status::ABORTED;
        }
        cmos_data.state = PersistentApplicationState::Idle;
    }
    drop(cmos_data);

    let mut corpus = Vec::new();
    corpus.push(Sample::default());
    trace!("Starting executor...");
    let mut executor = match SampleExecutor::new(&excluded_addresses.addresses) {
        Ok(executor) => executor,
        Err(e) => {
            println!("Failed to create executor: {:?}", e);
            return Status::ABORTED;
        }
    };
    info!("Doing hypervisor selfcheck");
    if !executor.selfcheck() {
        println!("Executor selfcheck failed");
        return Status::ABORTED;
    }
    info!("Executor selfcheck success");
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
    info!("Collecting ground truth coverage");
    executor.execute_sample(&corpus[0], &mut execution_result, &mut cmos);
    let ground_truth_coverage = execution_result.coverage.clone();
    if execution_result.events.len() > 0 {
        error!("Initial sample execution had events");
        for event in &execution_result.events {
            println!("{:#?}", event);
        }
        return Status::ABORTED;
    }

    // Main fuzzing loop
    loop {
        global_stats.iteration_count += 1;

        // Select a sample from the corpus
        let sample = global_scores
            .choose_sample_mut(&mut corpus, &mut random)
            .expect("There is at least one sample");

        // Mutate
        let (mutation, new_sample) = mutator.mutate_sample(sample, &mut random);

        // Execute
        executor.execute_sample(&new_sample, &mut execution_result, &mut cmos);

        // Handle events
        for event in &execution_result.events {
            match event {
                ExecutionEvent::CoverageCollectionError { error } => {
                    error!(
                        "Failed to collect coverage: {:?}. This should not have happened.",
                        error
                    );
                }
                ExecutionEvent::VmExitMismatchCoverageCollection {
                    addresses,
                    expected_exit,
                    actual_exit,
                } => {
                    error!("Expected result {:#?} but got {:#?}. This should not have happened. This is a ucode bug or implementation problem!", expected_exit, actual_exit);
                    println!("We were hooking the following addresses:");
                    #[cfg(not(feature = "__debug_bochs_pretend"))]
                    for address in addresses.iter() {
                        println!(" - {:?}", address);
                    }
                }
                ExecutionEvent::VmStateMismatchCoverageCollection {
                    addresses,
                    exit,
                    expected_state,
                    actual_state,
                } => {
                    error!("Expected architectural state x but got y. This should not have happened. This is a ucode bug or implementation problem!");
                    println!("We were hooking the following addresses:");
                    #[cfg(not(feature = "__debug_bochs_pretend"))]
                    for address in addresses.iter() {
                        println!(" - {:?}", address);
                    }
                    println!("We exited with {:#?}", exit);
                    println!("The difference was:");
                    for (field, expected, result) in expected_state.difference(actual_state) {
                        println!(
                            " - {:?}: expected {:x?}, got {:x?}",
                            field, expected, result
                        );
                    }
                }
            }
        }

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

        // todo remove
        if global_stats.iteration_count > 100 {
            break;
        }
    }

    warn!("Exiting...");
    drop(cmos);

    Status::SUCCESS
}

fn subtract_iter_btree<
    'a,
    'b,
    K: Ord + Display,
    V: SaturatingSub + Copy + Display + ConstZero + Ord,
>(
    original: &'a mut BTreeMap<K, V>,
    subtract_this: &'b BTreeMap<K, V>,
) {
    original.retain(|k, v| {
        if let Some(subtract_v) = subtract_this.get(k) {
            if *v < *subtract_v {
                error!(
                    "Reducing coverage count for more than ground truth: {k} : {v} < {subtract_v}"
                );
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
        if self.iteration_count % 1000 == 0 || self.iterations_since_last_gain == 0 {
            self.print();
        }
    }
    pub fn print(&self) {
        println!(
            "===============================================\nIteration: {:e}",
            self.iteration_count
        );
        println!(" - since last: {:e}", self.iterations_since_last_gain);
        println!(" - coverage: {}", self.coverage_sofar);
    }

    pub fn annonce_new_sample(&self, sample: &Sample, new_coverage: usize) {
        println!("New sample added to corpus");
        println!("The sample increased coverage by: {}", new_coverage);
        disassemble_code(&sample.code_blob);
    }
}

fn prepare_gdb() {
    let image_handle = uefi::boot::image_handle();
    let image_proto = uefi::boot::open_protocol_exclusive::<LoadedImage>(image_handle);

    if let Ok(image_proto) = image_proto {
        let (base, _size) = image_proto.info();
        debug!("Loaded image base: 0x{:x}", base as usize);
    }
}

struct ExcludedAddresses {
    addresses: BTreeSet<usize>,
}

impl ExcludedAddresses {
    const FILENAME: &'static str = "blacklist.txt";
    fn filename() -> Result<CString16, Error> {
        CString16::try_from(Self::FILENAME)
            .map_err(|_| uefi::Error::from(uefi::Status::UNSUPPORTED))
    }
    pub fn load_file() -> Result<Self, uefi::Error> {
        let mut proto = uefi::boot::get_image_file_system(uefi::boot::image_handle())?;
        let mut root_dir = proto.open_volume()?;
        let file = match root_dir.open(
            Self::filename()?.as_ref(),
            FileMode::Read,
            FileAttribute::empty(),
        ) {
            Ok(file) => file,
            Err(e) => {
                return if e.status() == Status::NOT_FOUND {
                    Ok(Self {
                        addresses: BTreeSet::new(),
                    })
                } else {
                    Err(e)
                }
            }
        };
        let mut regular_file = file
            .into_regular_file()
            .ok_or_else(|| uefi::Error::from(uefi::Status::UNSUPPORTED))?;

        let mut buffer = [0u8; 4096];
        let mut data = String::new();

        loop {
            let read = regular_file.read(&mut buffer)?;

            if read == 0 {
                break;
            }

            for i in 0..read {
                data.push(buffer[i] as char);
            }
        }

        let excluded = data
            .lines()
            .filter(|line| !(line.starts_with("//") || line.starts_with("#") || line.is_empty()))
            .filter_map(|line| {
                usize::from_str_radix(line, 16)
                    .map_err(|_| uefi::Error::from(uefi::Status::UNSUPPORTED))
                    .ok()
            });

        Ok(Self {
            addresses: BTreeSet::from_iter(excluded),
        })
    }

    pub fn exclude_address<A: Into<usize>>(&mut self, address: A) {
        self.addresses.insert(address.into());
    }

    pub fn save_file(&self) -> Result<(), uefi::Error> {
        let mut proto = uefi::boot::get_image_file_system(uefi::boot::image_handle())?;
        let mut root_dir = proto.open_volume()?;
        let file = root_dir.open(
            Self::filename().unwrap().as_ref(),
            FileMode::CreateReadWrite,
            FileAttribute::empty(),
        )?;
        let mut regular_file = file
            .into_regular_file()
            .ok_or_else(|| uefi::Error::from(uefi::Status::UNSUPPORTED))?;
        let mut data = String::new();
        for address in &self.addresses {
            data.push_str(&format!("{:04x}\n", address));
        }
        regular_file
            .write(data.as_bytes())
            .map_err(|_| uefi::Error::from(uefi::Status::WARN_WRITE_FAILURE))?;
        regular_file.flush()?;

        root_dir.flush()?;
        root_dir.close();

        Ok(())
    }
}
