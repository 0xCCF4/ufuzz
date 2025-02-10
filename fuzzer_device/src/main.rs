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
use fuzzer_device::executor::{
    ExecutionEvent, ExecutionResult, ExecutionSampleResult, SampleExecutor,
};
use fuzzer_device::genetic_breeding::{GeneticPool, GeneticPoolSettings};
use fuzzer_device::mutation_engine::InstructionDecoder;
use fuzzer_device::{
    disassemble_code, PersistentApplicationData, PersistentApplicationState, Trace,
};
use hypervisor::state::StateDifference;
use log::{debug, error, info, trace, warn};
use num_traits::{ConstZero, SaturatingSub};
use rand_core::SeedableRng;
use uefi::proto::loaded_image::LoadedImage;
use uefi::proto::media::file::{File, FileAttribute, FileMode};
#[cfg(feature = "device_bochs")]
use uefi::runtime::ResetType;
use uefi::{entry, println, CString16, Error, Status};

const MAX_ITERATIONS: usize = 1000;

#[entry]
unsafe fn main() -> Status {
    // todo setup idt

    uefi::helpers::init().unwrap();
    println!("Hello world!");
    println!(
        "Program version: {:08x}",
        PersistentApplicationData::this_app_version()
    );

    prepare_gdb();

    uefi::boot::stall(1000000);

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

    let mut random = random_source(0);

    // Declared global to avoid re-allocation
    let mut execution_result = ExecutionResult::default();

    // Coverage sofar
    let mut coverage_sofar = BTreeSet::<UCInstructionAddress>::new();

    // Global stats
    let mut global_stats = GlobalStats::default();

    // Samples
    let mut genetic_pool =
        GeneticPool::new_random_population(GeneticPoolSettings::default(), &mut random);

    // Execute initial sample to get ground truth coverage
    info!("Collecting ground truth coverage");
    let _ = executor.execute_sample(&[], &mut execution_result, &mut cmos, &mut random);
    let ground_truth_coverage = execution_result.coverage.clone();
    if execution_result.events.len() > 0 {
        error!("Initial sample execution had events");
        initial_execution_events_to_file(&execution_result);
        for event in &execution_result.events {
            println!("{:#?}", event);
        }
        //return Status::ABORTED;
    }
    info!(
        "Ground truth coverage collected: {:x?}",
        ground_truth_coverage
    );

    let mut trace_scratchpad = Trace::default();
    let mut decoder = InstructionDecoder::new();

    // Main fuzzing loop
    loop {
        for sample in genetic_pool.all_samples_mut() {
            global_stats.iteration_count += 1;

            // Execute
            let ExecutionSampleResult { serialized_sample } = executor.execute_sample(
                sample.code(),
                &mut execution_result,
                &mut cmos,
                &mut random,
            );

            // Handle events
            for event in &execution_result.events {
                match event {
                    ExecutionEvent::CoverageCollectionError { error } => {
                        error!(
                            "Failed to collect coverage: {:x?}. This should not have happened.",
                            error
                        );
                    }
                    ExecutionEvent::VmExitMismatchCoverageCollection {
                        addresses,
                        expected_exit,
                        actual_exit,
                    } => {
                        error!("CoverageCollection: Expected result {:#x?} but got {:#x?}. This should not have happened. This is a ucode bug or implementation problem!", expected_exit, actual_exit);
                        println!("We were hooking the following addresses:");
                        #[cfg(not(feature = "__debug_bochs_pretend"))]
                        for address in addresses.iter() {
                            println!(" - {:?}", address);
                        }
                        println!();
                    }
                    ExecutionEvent::VmStateMismatchCoverageCollection {
                        addresses,
                        exit,
                        expected_state,
                        actual_state,
                    } => {
                        error!("CoverageCollection: Expected architectural state x but got y. This should not have happened. This is a ucode bug or implementation problem!");
                        println!("We were hooking the following addresses:");
                        #[cfg(not(feature = "__debug_bochs_pretend"))]
                        for address in addresses.iter() {
                            println!(" - {:?}", address);
                        }
                        println!("We exited with {:#x?}", exit);
                        println!("The difference was:");
                        for (field, expected, result) in expected_state.difference(actual_state) {
                            println!(
                                " - {:?}: expected {:x?}, got {:x?}",
                                field, expected, result
                            );
                        }
                        println!();
                    }
                    ExecutionEvent::SerializedStateMismatch {
                        normal_exit,
                        normal_state,
                        serialized_exit,
                        serialized_state,
                    } => {
                        error!(
                            "SerializedExecution: Expected state x but got y. Is this a CPU bug?"
                        );
                        println!("In normal execution we exited with {:#x?}", normal_exit);
                        println!(
                            "We exited in serialized execution with {:#x?}",
                            serialized_exit
                        );
                        println!("Code:");
                        disassemble_code(sample.code());
                        trace_scratchpad.clear();
                        executor.trace_sample(sample.code(), &mut trace_scratchpad, 100);
                        println!("Normal-Trace: {:x?}", trace_scratchpad);
                        println!("Serialized code:");
                        disassemble_code(&serialized_sample.as_ref().unwrap());
                        trace_scratchpad.clear();
                        executor.trace_sample(
                            serialized_sample.as_ref().unwrap(),
                            &mut trace_scratchpad,
                            100,
                        );
                        println!("Serialized-Trace: {:x?}", trace_scratchpad);
                        println!("The difference was:");
                        for (field, expected, result) in normal_state.difference(serialized_state) {
                            let symbol = if field == "rip" { "*" } else { "-" };
                            println!(
                                " {} {:?}: normal {:x?}, serialized {:x?}",
                                symbol, field, expected, result
                            );
                        }
                        println!();
                    }
                    ExecutionEvent::SerializedExitMismatch {
                        normal_exit,
                        serialized_exit,
                    } => {
                        error!(
                            "SerializedExecution: Normal exit {:#x?} but got serialized exit {:#x?}. This should not have happened. Is this a bug?",
                            normal_exit, serialized_exit
                        );
                        println!("Code:");
                        disassemble_code(sample.code());
                        trace_scratchpad.clear();
                        executor.trace_sample(sample.code(), &mut trace_scratchpad, 100);
                        println!("Normal-Trace: {:x?}", trace_scratchpad);
                        println!("Serialized code:");
                        disassemble_code(&serialized_sample.as_ref().unwrap());
                        trace_scratchpad.clear();
                        executor.trace_sample(
                            serialized_sample.as_ref().unwrap(),
                            &mut trace_scratchpad,
                            100,
                        );
                        println!("Serialized-Trace: {:x?}", trace_scratchpad);
                        println!();
                    }
                }
            }

            // Rate
            sample.rate_from_execution(&execution_result, &mut decoder);

            let mut new_coverage = Vec::with_capacity(0);
            subtract_iter_btree(&mut execution_result.coverage, &ground_truth_coverage);
            for (address, count) in execution_result.coverage.iter() {
                if *count > 0 && !coverage_sofar.contains(address) {
                    new_coverage.push(address);
                }
            }

            global_stats.iterations_since_last_gain += 1;
            if new_coverage.len() > 0 {
                global_stats.annonce_new_sample(sample.code(), new_coverage.len());
                global_stats.iterations_since_last_gain = 0;
                corpus.push(sample.code().to_vec());
                coverage_sofar.extend(new_coverage);
                global_stats.coverage_sofar = coverage_sofar.len();
            }

            // Print status message
            global_stats.maybe_print();
        }

        // todo remove
        if global_stats.iteration_count > MAX_ITERATIONS {
            break;
        }

        genetic_pool.evolution(&mut random);
    }

    println!("Best performing programs are:");
    for code in genetic_pool.result().iter().take(4) {
        println!("-----------------");
        println!("Score: {}", code.rating().as_ref().unwrap_or_else(|| &0));
        disassemble_code(code.code());
        println!();
    }

    warn!("Exiting...");

    drop(cmos);

    #[cfg(feature = "device_bochs")]
    uefi::runtime::reset(ResetType::SHUTDOWN, Status::SUCCESS, None);

    Status::SUCCESS
}

fn initial_execution_events_to_file(result: &ExecutionResult) {
    let mut proto = uefi::boot::get_image_file_system(uefi::boot::image_handle()).unwrap();
    let mut root_dir = proto.open_volume().unwrap();
    let file = root_dir
        .open(
            CString16::try_from("initial_execution_events.txt")
                .unwrap()
                .as_ref(),
            FileMode::CreateReadWrite,
            FileAttribute::empty(),
        )
        .unwrap();
    let mut regular_file = file.into_regular_file().unwrap();
    let mut data = String::new();
    for event in &result.events {
        match event {
            ExecutionEvent::CoverageCollectionError { .. } => {
                data.push_str(&format!("{:#x?}\n", event));
            }
            ExecutionEvent::VmExitMismatchCoverageCollection { .. } => {
                data.push_str(&format!("{:#x?}\n", event));
            }
            ExecutionEvent::VmStateMismatchCoverageCollection {
                actual_state,
                expected_state,
                exit,
                addresses,
            } => {
                data.push_str("VmStateMismatchCoverageCollection\n");
                data.push_str(&format!("Addresses: {:#x?}\n", addresses));
                data.push_str(&format!("Exit: {:#x?}\n", exit));
                data.push_str("State mismatched:\n");
                for (field, expected, result) in expected_state.difference(actual_state) {
                    data.push_str(&format!(
                        " - {:?}: expected {:x?}, got {:x?}\n",
                        field, expected, result
                    ));
                }
            }
            ExecutionEvent::SerializedExitMismatch { .. } => {
                data.push_str(&format!("{:#x?}\n", event));
            }
            ExecutionEvent::SerializedStateMismatch {
                normal_exit: _,
                normal_state,
                serialized_exit,
                serialized_state,
            } => {
                data.push_str("SerializedStateMismatch\n");
                data.push_str(&format!("Exit: {:#x?}\n", serialized_exit));
                data.push_str("State mismatched:\n");
                for (field, expected, result) in normal_state.difference(serialized_state) {
                    data.push_str(&format!(
                        " - {:?}: normal {:x?}, serialized {:x?}\n",
                        field, expected, result
                    ));
                }
            }
        }
        data.push_str("\n\n");
    }
    data.push_str("Coverage:\n");
    for (address, count) in result.coverage.iter() {
        data.push_str(&format!("{:#x?}: {:#x?}\n", address, count));
    }
    data.push_str("\n\n");
    regular_file.write(data.as_bytes()).unwrap();
    regular_file.flush().unwrap();
    root_dir.flush().unwrap();
    root_dir.close();
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
        if self.iteration_count % 20 == 0 || self.iterations_since_last_gain == 0 {
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

    pub fn annonce_new_sample(&self, sample: &[u8], new_coverage: usize) {
        println!("New sample added to corpus");
        println!("The sample increased coverage by: {}", new_coverage);
        disassemble_code(sample);
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
