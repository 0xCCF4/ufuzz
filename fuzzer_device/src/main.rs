#![allow(incomplete_features)]
#![feature(generic_const_exprs)]
#![feature(stmt_expr_attributes)]
#![feature(proc_macro_hygiene)]
#![no_main]
#![no_std]

extern crate alloc;

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::{format, vec};
use core::cell::RefCell;
use core::ops::DerefMut;
use coverage::interface_definition::{CoverageCount, COM_INTERFACE_DESCRIPTION};
use data_types::addresses::{Address, UCInstructionAddress};
use fuzzer_data::decoder::InstructionDecoder;
use fuzzer_data::genetic_pool::{GeneticPool, GeneticPoolSettings, GeneticSampleRating};
use fuzzer_data::{
    genetic_pool, MemoryAccess, OtaC2D, OtaC2DTransport, OtaD2CTransport, ReportExecutionProblem,
    TraceResult,
};
use fuzzer_device::cmos::CMOS;
use fuzzer_device::controller_connection::{
    ConnectionError, ConnectionSettings, ControllerConnection,
};
use fuzzer_device::executor::{
    ExecutionEvent, ExecutionResult, ExecutionSampleResult, SampleExecutor,
};
use fuzzer_device::perf_monitor::PerfMonitor;
use fuzzer_device::{
    disassemble_code, PersistentApplicationData, PersistentApplicationState, StateTrace,
};
use hypervisor::state::VmState;
use itertools::Itertools;
use log::{debug, error, info, trace, warn, Level};
use performance_timing::measurements::MeasureValues;
#[cfg(feature = "__debug_performance_trace")]
use performance_timing::track_time;
use performance_timing::TimeMeasurement;
use rand_core::{RngCore, SeedableRng};
use uefi::boot::ScopedProtocol;
use uefi::proto::loaded_image::LoadedImage;
use uefi::proto::media::file::{File, FileAttribute, FileInfo, FileMode};
use uefi::{entry, println, CString16, Error, Status};
use uefi_raw::table::runtime::ResetType;
use uefi_raw::Ipv4Address;
use uefi_udp4::Ipv4AddressExt;
use x86::cpuid::cpuid;
use x86_perf_counter::PerformanceCounter;

#[cfg(feature = "device_brix")]
const P0_FREQ: f64 = 1.09e9;
#[cfg(feature = "device_bochs")]
const P0_FREQ: f64 = 1.0e9;

fn get_program_args() -> Vec<String> {
    let loaded_image_proto: ScopedProtocol<LoadedImage> =
        match uefi::boot::open_protocol_exclusive(uefi::boot::image_handle()) {
            Err(err) => {
                error!("Failed to open image protocol: {:?}", err);
                return Vec::new();
            }
            Ok(loaded_image_proto) => loaded_image_proto,
        };
    let options = match loaded_image_proto.load_options_as_bytes().map(|options| {
        let header = options as *const [u8] as *const u8;

        let description: *const u16 = header.cast();
        let mut description_data = Vec::new();

        for offset in 0..((options.len() - (description as usize - header as usize)) / 2) {
            let data: u16 = unsafe { description.add(offset).read() };
            description_data.push(data);
            if data == 0 {
                break;
            }
        }

        CString16::try_from(description_data).unwrap_or_else(|err| {
            error!("Failed to parse description: {:?}", err);
            CString16::new()
        })
    }) {
        None => {
            error!("No args set.");
            return Vec::new();
        }
        Some(options) => options,
    };

    let options = options.to_string();

    options
        .split_whitespace()
        .map(|e| e.to_string())
        .collect_vec()
}

#[entry]
unsafe fn main() -> Status {
    // todo setup idt

    uefi::helpers::init().unwrap();
    println!("Hello world!");
    println!(
        "Program version: {:08x}",
        PersistentApplicationData::this_app_version()
    );
    if let Err(err) = performance_timing::initialize(P0_FREQ) {
        error!("Failed to initialize performance timing: {:?}", err);
        return Status::ABORTED;
    }

    let program_args = get_program_args();
    println!("Args: {:?}", program_args);

    let mut connection_settings = ConnectionSettings::default();
    if program_args.len() < 5 {
        warn!("Using default connection settings. Provide at least <REMOTE_IP> <SOURCE_IP> <SUBNET_MASK> <PORT>")
    } else {
        fn parse_ip(s: &str) -> Option<Ipv4Address> {
            let parts: Vec<&str> = s.split('.').collect();
            if parts.len() != 4 {
                return None;
            }
            let mut bytes = [0u8; 4];
            for (i, part) in parts.iter().enumerate() {
                if let Ok(num) = part.parse::<u8>() {
                    bytes[i] = num;
                } else {
                    return None;
                }
            }
            Some(Ipv4Address::new(bytes[0], bytes[1], bytes[2], bytes[3]))
        }

        if let Some(remote_ip) = parse_ip(&program_args[1]) {
            connection_settings.remote_address = remote_ip;
        } else {
            warn!("Invalid remote IP address: {}", program_args[1]);
        }

        if let Some(source_ip) = parse_ip(&program_args[2]) {
            connection_settings.source_address = source_ip;
        } else {
            warn!("Invalid source IP address: {}", program_args[2]);
        }

        if let Some(subnet_mask) = parse_ip(&program_args[3]) {
            connection_settings.subnet_mask = subnet_mask;
        } else {
            warn!("Invalid subnet mask: {}", program_args[3]);
        }

        if let Ok(port) = program_args[4].parse::<u16>() {
            connection_settings.remote_port = port;
            connection_settings.source_port = port;
        } else {
            warn!("Invalid port number: {}", program_args[4]);
        }
    }
    println!("--------------");
    println!("Remote IP: {:?}", connection_settings.remote_address);
    println!("Source IP: {:?}", connection_settings.source_address);
    println!("Subnet Mask: {:?}", connection_settings.subnet_mask);
    println!("Remote Port: {}", connection_settings.remote_port);
    println!("Source Port: {}", connection_settings.source_port);
    println!("--------------");

    prepare_gdb();

    uefi::boot::stall(1000000);

    let perf_monitor = match PerfMonitor::new("perf.json") {
        Ok(x) => x,
        Err(e) => {
            error!("Failed to create perf monitor: {:?}", e);
            return Status::ABORTED;
        }
    };

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

        match ControllerConnection::connect(&connection_settings) {
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
    let mut executor =
        match SampleExecutor::new(Rc::clone(&excluded_addresses), &COM_INTERFACE_DESCRIPTION) {
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
        // Execute initial sample to get ground truth coverage
        let mut random = random_source(0);
        let mut execution_result = ExecutionResult::default();
        let mut state_trace_scratchpad_normal = StateTrace::default();
        let mut state_trace_scratchpad_serialized = StateTrace::default();
        let mut state_trace_with_memory: StateTrace<(VmState, Vec<MemoryAccess>)> =
            StateTrace::default();
        let mut decoder = InstructionDecoder::new();

        if let Err(err) = udp.send(OtaD2CTransport::ResetSession) {
            error!("Failed to send reset-session: {:?}", err);
        }

        trace!("Waiting for command...");
        #[cfg_attr(
            feature = "__debug_performance_trace",
            track_time("fuzzer_device::main_loop")
        )]
        loop {
            #[cfg(feature = "__debug_performance_trace")]
            if let Err(err) = perf_monitor.try_update_save_file() {
                error!("Failed to save perf monitor values: {:?}", err);
                let _ = udp.log_reliable(
                    Level::Error,
                    format!("Failed to save perf monitor values: {:?}", err),
                );
            }

            let packet = match udp.receive(None) {
                Ok(None) => continue,
                Ok(Some(packet)) => packet,
                Err(err) => {
                    error!("Failed to receive packet: {:?}", err);
                    let _ = udp
                        .log_reliable(Level::Error, format!("Failed to receive packet: {:?}", err));
                    continue;
                }
            };

            let packet = match packet {
                OtaC2D::Transport { content, .. } => content,
                _ => continue,
            };

            match packet {
                OtaC2DTransport::GetCapabilities { leaf, node } => {
                    let vendor_str = x86::cpuid::CpuId::new()
                        .get_vendor_info()
                        .map(|v| v.to_string())
                        .unwrap_or("---".to_string());
                    let processor_version = cpuid!(leaf, node);
                    let capabilities = OtaD2CTransport::Capabilities {
                        coverage_collection: executor.supports_coverage_collection(),
                        manufacturer: vendor_str,
                        pmc_number: PerformanceCounter::number_of_counters(),
                        processor_version_eax: processor_version.eax,
                        processor_version_ebx: processor_version.ebx,
                        processor_version_ecx: processor_version.ecx,
                        processor_version_edx: processor_version.edx,
                    };
                    if let Err(err) = udp.send(capabilities) {
                        error!("Failed to send capabilities: {:?}", err);
                        let _ = udp.log_reliable(
                            Level::Error,
                            format!("Failed to send capabilities: {:?}", err),
                        );
                    }
                }
                OtaC2DTransport::Blacklist { address } =>
                #[cfg_attr(
                    feature = "__debug_performance_trace",
                    track_time("fuzzer_device::main_loop::blacklist-set")
                )]
                {
                    let mut excluded_addresses_mut = excluded_addresses.borrow_mut();

                    for addr in address {
                        excluded_addresses_mut.insert(addr);
                    }
                    drop(excluded_addresses_mut);
                    executor.update_excluded_addresses();
                }
                OtaC2DTransport::ResetBlacklist => {
                    excluded_addresses.borrow_mut().clear();
                    executor.update_excluded_addresses();
                    for _ in 0..50 {
                        match (ExcludedAddresses {
                            addresses: BTreeSet::new(),
                        })
                        .save_file()
                        {
                            Ok(_) => break,
                            Err(err) => {
                                error!("Failed to reset excluded-addresses: {:?}", err);
                                let _ = udp.log_reliable(
                                    Level::Error,
                                    format!("Failed to reset excluded-addresses: {:?}", err),
                                );
                            }
                        }
                    }
                }
                OtaC2DTransport::DidYouExcludeAnAddressLastRun => {
                    let blacklisted = OtaD2CTransport::LastRunBlacklisted {
                        address: excluded_last_run,
                    };
                    if let Err(err) = udp.send(blacklisted) {
                        error!("Failed to send blacklisted: {:?}", err);
                        let _ = udp.log_reliable(
                            Level::Error,
                            format!("Failed to send blacklisted: {:?}", err),
                        );
                    }
                }
                OtaC2DTransport::Reboot => {
                    #[cfg(feature = "device_bochs")]
                    uefi::runtime::reset(ResetType::SHUTDOWN, Status::SUCCESS, None);
                    #[cfg(not(feature = "device_bochs"))]
                    break;
                }
                OtaC2DTransport::AreYouThere => {}
                OtaC2DTransport::GiveMeYourBlacklistedAddresses =>
                #[cfg_attr(
                    feature = "__debug_performance_trace",
                    track_time("fuzzer_device::main_loop::blacklist-get")
                )]
                {
                    let borrow = excluded_addresses.borrow();
                    for blacklisted in borrow.iter().cloned().collect::<Vec<u16>>().chunks(200) {
                        let blacklisted = OtaD2CTransport::BlacklistedAddresses {
                            addresses: blacklisted.to_vec(),
                        };
                        if let Err(err) = udp.send(blacklisted) {
                            error!("Failed to send blacklisted addresses: {:?}", err);
                            let _ = udp.log_reliable(
                                Level::Error,
                                format!("Failed to send blacklisted addresses: {:?}", err),
                            );
                        }
                    }
                }
                OtaC2DTransport::ReportPerformanceTiming => {
                    let measurements = perf_monitor.measurement_data.accumulate();
                    for chunk in &measurements.into_iter().chunks(5) {
                        let data: BTreeMap<String, MeasureValues<f64>> = chunk
                            .map(|(k, v)| (k, MeasureValues::<f64>::from(&v)))
                            .collect::<BTreeMap<String, MeasureValues<f64>>>();
                        let measurements = OtaD2CTransport::PerformanceTiming {
                            measurements: data.clone(),
                        };
                        if let Err(err) = udp.send(measurements.clone()) {
                            if matches!(err, ConnectionError::PacketTooLarge) {
                                for m in data.into_iter() {
                                    if let Err(err) = udp.send(OtaD2CTransport::PerformanceTiming {
                                        measurements: vec![m].into_iter().collect(),
                                    }) {
                                        error!("Failed to send performance timing: {:?}", err);
                                        let _ = udp.log_reliable(
                                            Level::Error,
                                            format!("Failed to send performance timing: {:?}", err),
                                        );
                                    }
                                }
                            } else {
                                error!("Failed to send performance timing: {:?}", err);
                                let _ = udp.log_reliable(
                                    Level::Error,
                                    format!("Failed to send performance timing: {:?}", err),
                                );
                            }
                        }
                    }
                }
                OtaC2DTransport::SetRandomSeed { seed } => {
                    random = random_source(seed);
                }
                OtaC2DTransport::ExecuteSample { code, coverage } =>
                #[cfg_attr(
                    feature = "__debug_performance_trace",
                    track_time("fuzzer_device::main_loop::execute_sample")
                )]
                {
                    println!("Executing sample");
                    let _ = udp.log_unreliable(Level::Trace, "Executing sample");

                    let ExecutionSampleResult { serialized_sample } = executor.execute_sample(
                        &code,
                        &mut execution_result,
                        &mut cmos,
                        &mut random,
                        Some(&mut udp),
                        coverage,
                    );

                    let events = execution_result
                        .events
                        .iter()
                        .cloned()
                        .filter_map(Option::<ReportExecutionProblem>::from);
                    for event in (&events
                        .into_iter()
                        .chunks(ReportExecutionProblem::MAX_PER_PACKET))
                        .into_iter()
                        .map(|chunk| chunk.collect_vec())
                    {
                        if let Err(err) = udp.send(OtaD2CTransport::ExecutionEvents(event.clone()))
                        {
                            error!("Failed to send event: {:?}", err);
                            let _ = udp.log_reliable(
                                Level::Error,
                                format!("Failed to send event: {:?}", err),
                            );

                            if matches!(err, ConnectionError::PacketTooLarge) {
                                for event in event {
                                    if let Err(err) =
                                        udp.send(OtaD2CTransport::ExecutionEvents(vec![event]))
                                    {
                                        error!("Failed to send event: {:?}", err);
                                        let _ = udp.log_reliable(
                                            Level::Error,
                                            format!("Failed to send event: {:?}", err),
                                        );
                                    }
                                }
                            }
                        }
                    }
                    for event in &execution_result.events {
                        if let ExecutionEvent::SerializedMismatch {
                            serialized_exit: _,
                            serialized_state: _,
                        } = event
                        {
                            state_trace_scratchpad_normal.clear();
                            state_trace_scratchpad_serialized.clear();

                            executor.state_trace_sample(
                                &code,
                                &mut state_trace_scratchpad_normal,
                                300,
                            );
                            executor.state_trace_sample(
                                serialized_sample.as_ref().unwrap(),
                                &mut state_trace_scratchpad_serialized,
                                300,
                            );

                            let difference = state_trace_scratchpad_normal
                                .first_difference_no_addresses(&state_trace_scratchpad_serialized);
                            match difference {
                                None => {
                                    if let Err(_err) =
                                        udp.send(OtaD2CTransport::ExecutionEvents(vec![
                                            ReportExecutionProblem::VeryLikelyBug,
                                        ]))
                                    {
                                        if let Err(err) =
                                            udp.send(OtaD2CTransport::ExecutionEvents(
                                                vec![ReportExecutionProblem::VeryLikelyBug].into(),
                                            ))
                                        {
                                            error!("Failed to send serial event: {:?}", err);
                                            let _ = udp.log_reliable(
                                                Level::Error,
                                                format!("Failed to send serial event: {:?}", err),
                                            );
                                        }
                                    }
                                }
                                Some(index) => {
                                    for i in index
                                        ..state_trace_scratchpad_normal
                                            .len()
                                            .max(state_trace_scratchpad_serialized.len())
                                    {
                                        let normal = state_trace_scratchpad_normal.get(i);
                                        let serialized = state_trace_scratchpad_serialized.get(i);

                                        let event = ReportExecutionProblem::StateTraceMismatch {
                                            index: i as u64,
                                            normal: normal.cloned(),
                                            serialized: serialized.cloned(),
                                        };

                                        if let Err(_err) =
                                            udp.send(OtaD2CTransport::ExecutionEvents(vec![
                                                event.clone()
                                            ]))
                                        {
                                            if let Err(err) = udp
                                                .send(OtaD2CTransport::ExecutionEvents(vec![event]))
                                            {
                                                error!("Failed to send event: {:?}", err);
                                                let _ = udp.log_reliable(
                                                    Level::Error,
                                                    format!("Failed to send event: {:?}", err),
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    let _guard = TimeMeasurement::begin("fuzzer_device::main_loop::send_results");

                    for cov in execution_result
                        .coverage
                        .iter()
                        .map(|(k, v)| (k.address() as u16, *v))
                        .collect_vec()
                        .chunks(200)
                    {
                        if let Err(err) = udp.send(OtaD2CTransport::Coverage {
                            coverage: cov.to_vec(),
                        }) {
                            error!("Failed to send coverage: {:?}", err);
                        }
                    }

                    if let Err(err) = udp.send(OtaD2CTransport::Serialized {
                        serialized: serialized_sample,
                    }) {
                        error!("Failed to send serialized: {:?}", err);
                    }

                    if let Err(err) = udp.send(OtaD2CTransport::ExecutionResult {
                        exit: execution_result.exit.clone(),
                        state: execution_result.state.clone(),
                        fitness: rate_sample_from_execution(&code, &mut decoder, &execution_result),
                    }) {
                        error!("Failed to send execution result: {:?}", err);
                    }

                    drop(_guard);
                }
                OtaC2DTransport::TraceSample {
                    code,
                    max_iterations,
                    record_memory_access,
                } => {
                    if record_memory_access {
                        let exit = executor.state_trace_sample_mem(
                            code.as_slice(),
                            &mut state_trace_with_memory,
                            max_iterations.min(u16::MAX as u64) as usize,
                        );
                        for (i, (state, memory_accesses)) in
                            state_trace_with_memory.state.iter().enumerate()
                        {
                            if let Err(err) =
                                udp.send(OtaD2CTransport::TraceResult(TraceResult::Running {
                                    memory_accesses: memory_accesses.clone(),
                                    state: state.clone(),
                                    index: i as u16,
                                }))
                            {
                                error!("Failed to send trace result: {:?}", err);
                            }
                        }
                        if let Err(err) = udp.send(OtaD2CTransport::TraceResult(exit.into())) {
                            error!("Failed to send trace exit: {:?}", err);
                        }
                        state_trace_scratchpad_normal.clear();
                    } else {
                        let exit = executor.state_trace_sample(
                            code.as_slice(),
                            &mut state_trace_scratchpad_normal,
                            max_iterations.min(u16::MAX as u64) as usize,
                        );
                        for (i, state) in state_trace_scratchpad_normal.state.iter().enumerate() {
                            if let Err(err) =
                                udp.send(OtaD2CTransport::TraceResult(TraceResult::Running {
                                    memory_accesses: Vec::new(),
                                    state: state.clone(),
                                    index: i as u16,
                                }))
                            {
                                error!("Failed to send trace result: {:?}", err);
                            }
                        }
                        if let Err(err) = udp.send(OtaD2CTransport::TraceResult(exit.into())) {
                            error!("Failed to send trace exit: {:?}", err);
                        }
                        state_trace_scratchpad_normal.clear();
                    }
                }
                OtaC2DTransport::UCodeSpeculation { .. }
                | OtaC2DTransport::TestIfPMCStable { .. } => {
                    let _ = udp.log_reliable(Level::Error, "Method not supported");
                    error!("Method not supported");
                }
                OtaC2DTransport::RunScenario(_name, _payload) => {
                    /*info!("Running scenario {}", name);
                    let result = poc_agent::execute(&name, payload.as_slice());
                    if let Err(err) = udp.send(OtaD2CTransport::ScenarioResult(name, result)) {
                        error!("Failed to send result: {:?}", err);
                    }*/
                    let _ = udp.log_reliable(Level::Error, "Method not supported");
                    error!("Method not supported");
                }
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

#[allow(unused_variables)]
#[allow(unused_mut)]
#[allow(dead_code)]
fn genetic_pool_fuzzing(
    executor: &mut SampleExecutor,
    cmos: &mut CMOS<PersistentApplicationData>,
    seed: u64,
    pool_settings: GeneticPoolSettings,
    evolutions: u64,
    mut network: Option<&mut ControllerConnection>,
) -> Vec<genetic_pool::Sample> {
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
    let ground_truth_coverage =
        ground_truth_coverage(executor, &mut execution_result, cmos, &mut random);

    let mut state_trace_scratchpad_normal: StateTrace<VmState> = StateTrace::default();
    let mut state_trace_scratchpad_serialized: StateTrace<VmState> = StateTrace::default();
    let mut state_trace_with_memory: StateTrace<(VmState, Vec<MemoryAccess>)> =
        StateTrace::default();
    let mut decoder = InstructionDecoder::new();

    // Main fuzzing loop
    for evolution_count in 0..evolutions {
        for sample in genetic_pool.all_samples_mut() {
            global_stats.iteration_count += 1;

            // Execute
            let ExecutionSampleResult { serialized_sample } = executor.execute_sample(
                sample.code(),
                &mut execution_result,
                cmos,
                &mut random,
                None,
                true,
            );

            // Handle events
            for event in &execution_result.events {
                #[cfg(feature = "__debug_print_events")]
                match event {
                    ExecutionEvent::CoverageCollectionError { error } => {
                        error!(
                            "Failed to collect coverage: {:x?}. This should not have happened.",
                            error
                        );
                    }
                    ExecutionEvent::VmMismatchCoverageCollection {
                        address,
                        coverage_exit,
                        coverage_state,
                    } => {
                        error!("CoverageCollection: Expected architectural state x but got y. This should not have happened. This is a ucode bug or implementation problem!");
                        #[cfg(not(feature = "__debug_bochs_pretend"))]
                        println!("We were hooking the following address: {}", address);
                        println!("We exited with {:#x?}", coverage_exit);
                        println!("We should have exited with {:#x?}", execution_result.exit);
                        if let Some(coverage_state) = coverage_state {
                            println!("The state difference was:");
                            for (field, expected, result) in
                                execution_result.state.difference(coverage_state)
                            {
                                println!(
                                    " - {:?}: expected {:x?}, got {:x?}",
                                    field, expected, result
                                );
                            }
                        }
                        println!();
                    }
                    ExecutionEvent::SerializedMismatch {
                        serialized_exit,
                        serialized_state,
                    } => {
                        error!(
                            "SerializedExecution: Expected state x but got y. Is this a CPU bug?"
                        );
                        println!(
                            "In normal execution we exited with {:#x?}",
                            execution_result.exit
                        );
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
                        if let Some(serialized_state) = serialized_state {
                            println!("The difference was:");
                            for (field, expected, result) in
                                execution_result.state.difference(serialized_state)
                            {
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
                                None => {
                                    println!("No difference found -> This is very odd! CPU bug?")
                                }
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

                                        if let (Some(normal), Some(serialized)) =
                                            (normal, serialized)
                                        {
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
                        }
                        println!();
                    }
                }
            }
            if let Some(net) = network.as_mut() {
                let events = execution_result
                    .events
                    .iter()
                    .cloned()
                    .filter_map(Option::<ReportExecutionProblem>::from);
                for event in (&events
                    .into_iter()
                    .chunks(ReportExecutionProblem::MAX_PER_PACKET))
                    .into_iter()
                    .map(|chunk| chunk.collect_vec())
                {
                    if let Err(err) = net.send(OtaD2CTransport::ExecutionEvents(event)) {
                        error!("Failed to send event: {:?}", err);
                    }
                }
            }

            // Rate
            sample.rating = Some(rate_sample_from_execution(
                sample.code(),
                &mut decoder,
                &execution_result,
            ));

            let mut new_coverage = Vec::with_capacity(0);
            //subtract_iter_btree(&mut execution_result.coverage, &ground_truth_coverage);
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
            genetic_pool.evolution(&mut random, true);
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

fn ground_truth_coverage<R: RngCore>(
    executor: &mut SampleExecutor,
    execution_result: &mut ExecutionResult,
    cmos: &mut CMOS<PersistentApplicationData>,
    random: &mut R,
) -> BTreeMap<UCInstructionAddress, CoverageCount> {
    info!("Collecting ground truth coverage");
    let _ = executor.execute_sample(&[], execution_result, cmos, random, None, true);
    let ground_truth_coverage = execution_result.coverage.clone();
    if execution_result.events.len() > 0 {
        error!("Initial sample execution had events");
        #[cfg(feature = "__debug_print_events")]
        for event in &execution_result.events {
            println!("{:#?}", event);
        }
        //return Status::ABORTED;
    }
    info!(
        "Ground truth coverage collected: {:x?}",
        ground_truth_coverage
    );
    ground_truth_coverage
}

pub fn rate_sample_from_execution(
    code: &[u8],
    decoder: &mut InstructionDecoder,
    execution_result: &ExecutionResult,
) -> GeneticSampleRating {
    let number_of_instructions = decoder.decode(code, 0).len();

    // expects existing entries to all have values >0
    let unique_address_coverage = execution_result.coverage.keys().count();
    let total_address_coverage = execution_result
        .coverage
        .values()
        .map(|val| *val as u32)
        .sum();
    let unique_trace_addresses = execution_result
        .trace
        .hit
        .keys()
        .filter(|&ip| *ip < code.len() as u64)
        .count();
    let program_utilization = (f32::clamp(
        unique_trace_addresses as f32 / number_of_instructions as f32,
        0.0,
        1.0,
    ) * 100.0) as usize;
    let loop_count = execution_result.trace.hit.values().max().unwrap_or(&1) - 1;

    GeneticSampleRating {
        unique_address_coverage: unique_address_coverage as u16,
        total_address_coverage,
        program_utilization: program_utilization as u8,
        loop_count,
    }
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

#[derive(Debug, Default)]
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
                if buffer[i] != 0 {
                    data.push(buffer[i] as char);
                }
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

        let mut buf = Vec::new();
        let fileinfo_size = regular_file
            .get_info::<FileInfo>(&mut buf)
            .unwrap_err()
            .data()
            .ok_or(uefi::Error::from(uefi::Status::UNSUPPORTED))?;
        buf.resize(fileinfo_size, 0);
        let file_size = regular_file
            .get_info::<FileInfo>(&mut buf)
            .map_err(|_| uefi::Error::from(uefi::Status::WARN_WRITE_FAILURE))?
            .file_size();
        let current_position = regular_file.get_position()?;

        for _ in 0..(file_size - current_position).div_ceil(128) {
            regular_file
                .write(&[0; 128])
                .map_err(|_| uefi::Error::from(uefi::Status::WARN_WRITE_FAILURE))?;
        }

        root_dir.flush()?;
        root_dir.close();

        Ok(())
    }
}
