#![allow(incomplete_features)]
#![feature(generic_const_exprs)]
#![no_main]
#![no_std]

extern crate alloc;

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;
use core::fmt::Display;
use core::ops::DerefMut;
use data_types::addresses::UCInstructionAddress;
use fuzzer_data::{OtaC2D, OtaC2DTransport, OtaD2CTransport, OtaD2CUnreliable};
use fuzzer_device::cmos::CMOS;
use fuzzer_device::controller_connection::{ConnectionSettings, ControllerConnection};
use fuzzer_device::executor::{
    ExecutionEvent, ExecutionResult, ExecutionSampleResult, SampleExecutor,
};
use fuzzer_device::genetic_breeding::{GeneticPool, GeneticPoolSettings};
use fuzzer_device::mutation_engine::InstructionDecoder;
use fuzzer_device::{
    disassemble_code, genetic_breeding, PersistentApplicationData, PersistentApplicationState,
    StateTrace, Trace,
};
use hypervisor::state::StateDifference;
use log::{debug, error, info, trace, warn};
use num_traits::{ConstZero, SaturatingSub};
use rand_core::SeedableRng;
use uefi::proto::loaded_image::LoadedImage;
use uefi::proto::media::file::{File, FileAttribute, FileMode};
use uefi::{entry, println, CString16, Error, Status};
use uefi_raw::table::runtime::ResetType;
use x86::cpuid::cpuid;

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
    let mut excluded_last_run = None;
    if let PersistentApplicationState::CollectingCoverage(address) = cmos_data.state {
        // failed to collect coverage from that address
        excluded_addresses.exclude_address(address);
        excluded_last_run = Some(address);
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
    let excluded_last_run = excluded_last_run;
    drop(cmos_data);

    #[cfg(not(feature = "device_bochs"))]
    let udp: Option<ControllerConnection> = {
        trace!("Connecting to UDP");

        match ControllerConnection::connect(&ConnectionSettings::default()) {
            Ok(udp) => Some(udp),
            Err(err) => {
                error!("Failed to connect to controller: {:?}", err);
                #[cfg(not(feature = "device_bochs"))]
                return Status::ABORTED;
                #[cfg(feature = "device_bochs")]
                None
            }
        }
    };

    trace!("Excluded addresses: {:x?}", excluded_addresses.addresses);
    let excluded_addresses = Rc::new(RefCell::new(excluded_addresses.addresses));

    trace!("Starting executor...");
    let mut executor = match SampleExecutor::new(Rc::clone(&excluded_addresses)) {
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

    // Bootstrap finished
    // ready to accept commands

    #[cfg(feature = "device_bochs")]
    {
        genetic_pool_fuzzing(
            &mut executor,
            &mut cmos,
            0,
            GeneticPoolSettings::default(),
            10,
            None,
        );
    }

    #[cfg(not(feature = "device_bochs"))]
    if let Some(mut udp) = udp {
        trace!("Waiting for command...");
        loop {
            let packet = match udp.receive(None) {
                Ok(None) => continue,
                Ok(Some(packet)) => packet,
                Err(err) => {
                    error!("Failed to receive packet: {:?}", err);
                    continue;
                }
            };

            let packet = match packet {
                OtaC2D::Transport { content, .. } => content,
                _ => continue,
            };

            match packet {
                OtaC2DTransport::GetCapabilities => {
                    let vendor_str = x86::cpuid::CpuId::new()
                        .get_vendor_info()
                        .map(|v| v.to_string())
                        .unwrap_or("---".to_string());
                    let processor_version = cpuid!(0x1);
                    let capabilities = OtaD2CTransport::Capabilities {
                        coverage_collection: executor.supports_coverage_collection(),
                        manufacturer: vendor_str,
                        processor_version_eax: processor_version.eax,
                        processor_version_ebx: processor_version.ebx,
                        processor_version_ecx: processor_version.ecx,
                        processor_version_edx: processor_version.edx,
                    };
                    if let Err(err) = udp.send(capabilities) {
                        error!("Failed to send capabilities: {:?}", err);
                    }
                }
                OtaC2DTransport::Blacklist { address } => {
                    let mut excluded_addresses_mut = excluded_addresses.borrow_mut();

                    for addr in address {
                        excluded_addresses_mut.insert(addr);
                    }
                    drop(excluded_addresses_mut);
                    executor.update_excluded_addresses();
                }
                OtaC2DTransport::DidYouExcludeAnAddressLastRun => {
                    let blacklisted = OtaD2CTransport::Blacklisted {
                        address: excluded_last_run,
                    };
                    if let Err(err) = udp.send(blacklisted) {
                        error!("Failed to send blacklisted: {:?}", err);
                    }
                }
                OtaC2DTransport::StartGeneticFuzzing {
                    seed,
                    evolutions,

                    population_size,
                    code_size,

                    random_solutions_each_generation,
                    keep_best_x_solutions,
                    random_mutation_chance,
                } => {
                    let result = genetic_pool_fuzzing(
                        &mut executor,
                        &mut cmos,
                        seed,
                        GeneticPoolSettings {
                            population_size,
                            code_size,
                            random_solutions_each_generation,
                            keep_best_x_solutions,
                            random_mutation_chance,
                        },
                        evolutions,
                        Some(&mut udp),
                    );

                    // todo use result
                    drop(result);

                    if let Err(err) = udp.send(OtaD2CTransport::FinishedGeneticFuzzing) {
                        error!("Failed to send finish fuzzing: {:?}", err);
                    }
                }
                OtaC2DTransport::Reboot => {
                    #[cfg(feature = "device_bochs")]
                    uefi::runtime::reset(ResetType::SHUTDOWN, Status::SUCCESS, None);
                    #[cfg(not(feature = "device_bochs"))]
                    break;
                }
                OtaC2DTransport::AreYouThere => {}
            }
        }
    } else {
        unreachable!("Since above we exit if udp is None and not bochs")
    }

    warn!("Exiting...");

    drop(cmos);

    #[cfg(feature = "device_bochs")]
    uefi::runtime::reset(ResetType::SHUTDOWN, Status::SUCCESS, None);
    #[cfg(not(feature = "device_bochs"))]
    uefi::runtime::reset(ResetType::COLD, Status::SUCCESS, None);
}

fn genetic_pool_fuzzing(
    executor: &mut SampleExecutor,
    cmos: &mut CMOS<PersistentApplicationData>,
    seed: u64,
    pool_settings: GeneticPoolSettings,
    evolutions: u64,
    mut network: Option<&mut ControllerConnection>,
) -> Vec<genetic_breeding::Sample> {
    let mut random = random_source(seed);

    // Declared global to avoid re-allocation
    let mut execution_result = ExecutionResult::default();

    // Coverage sofar
    let mut coverage_sofar = BTreeSet::<UCInstructionAddress>::new();

    // Global stats
    let mut global_stats = GlobalStats::default();

    // Samples
    let mut genetic_pool = GeneticPool::new_random_population(pool_settings, &mut random);

    // Execute initial sample to get ground truth coverage
    info!("Collecting ground truth coverage");
    let _ = executor.execute_sample(&[], &mut execution_result, cmos, &mut random);
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
    let mut state_trace_scratchpad_normal = StateTrace::default();
    let mut state_trace_scratchpad_serialized = StateTrace::default();
    let mut decoder = InstructionDecoder::new();

    // Main fuzzing loop
    for evolution_count in 0..evolutions {
        for sample in genetic_pool.all_samples_mut() {
            global_stats.iteration_count += 1;

            // Alive message
            // todo make sending this message less often
            if let Some(net) = &mut network {
                if let Err(err) = net.send(OtaD2CUnreliable::Alive {
                    iteration: global_stats.iteration_count,
                }) {
                    warn!("Failed to send alive message: {:?}", err);
                }
            }

            // Execute
            let ExecutionSampleResult { serialized_sample } =
                executor.execute_sample(sample.code(), &mut execution_result, cmos, &mut random);

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
                        state_trace_scratchpad_normal.clear();
                        executor.state_trace_sample(
                            sample.code(),
                            &mut state_trace_scratchpad_normal,
                            100,
                        );
                        println!(
                            "Normal-Trace: {:x?}",
                            state_trace_scratchpad_normal.trace_vec()
                        );
                        println!("Serialized code:");
                        disassemble_code(&serialized_sample.as_ref().unwrap());
                        state_trace_scratchpad_serialized.clear();
                        executor.state_trace_sample(
                            serialized_sample.as_ref().unwrap(),
                            &mut state_trace_scratchpad_serialized,
                            100,
                        );
                        println!(
                            "Serialized-Trace: {:x?}",
                            state_trace_scratchpad_serialized.trace_vec()
                        );
                        println!("The difference was:");
                        for (field, expected, result) in normal_state.difference(serialized_state) {
                            let symbol = if field == "rip" { "*" } else { "-" };
                            println!(
                                " {} {:?}: normal {:x?}, serialized {:x?}",
                                symbol, field, expected, result
                            );
                        }
                        println!("Difference occurred at:");
                        let difference = state_trace_scratchpad_normal
                            .first_difference_no_addresses(&state_trace_scratchpad_serialized);
                        match difference {
                            None => println!("No difference found -> This is very odd! CPU bug?"),
                            Some(index) => {
                                for i in index
                                    ..state_trace_scratchpad_normal
                                        .len()
                                        .max(state_trace_scratchpad_serialized.len())
                                {
                                    let normal = state_trace_scratchpad_normal.get(i);
                                    let serialized = state_trace_scratchpad_serialized.get(i);

                                    if let Some(normal) = normal {
                                        println!(" -- {:x?} --", normal.standard_registers.rip);
                                    }

                                    if let (Some(normal), Some(serialized)) = (normal, serialized) {
                                        for (field, expected, result) in
                                            normal.difference(serialized)
                                        {
                                            let symbol = if field == "rip" { "*" } else { "-" };
                                            println!(
                                                " {} {:?}: normal {:x?}, serialized {:x?}",
                                                symbol, field, expected, result
                                            );
                                        }
                                    } else {
                                        println!("Execution for one of the traces stopped");
                                        break;
                                    }
                                }
                            }
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
                //corpus.push(sample.code().to_vec());
                coverage_sofar.extend(new_coverage);
                global_stats.coverage_sofar = coverage_sofar.len();
            }

            // Print status message
            global_stats.maybe_print();
        }

        if evolution_count + 1 < evolutions {
            genetic_pool.evolution(&mut random);
        }
    }

    let result = genetic_pool.result();

    println!("Best performing programs are:");
    for code in result.iter().take(4) {
        println!("-----------------");
        println!("Score: {:?}", code.rating);
        disassemble_code(code.code());
        println!();
    }

    result
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
    pub iteration_count: u64,
    pub iterations_since_last_gain: u64,
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

    let system_table = uefi::table::system_table_raw();

    if let Some(system_table) = system_table {
        debug!("System table: {:x?}", system_table.as_ptr() as usize);
    }
}

struct ExcludedAddresses {
    addresses: BTreeSet<u16>,
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
                u16::from_str_radix(line, 16)
                    .map_err(|_| uefi::Error::from(uefi::Status::UNSUPPORTED))
                    .ok()
            });

        Ok(Self {
            addresses: BTreeSet::from_iter(excluded),
        })
    }

    pub fn exclude_address<A: Into<u16>>(&mut self, address: A) {
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
