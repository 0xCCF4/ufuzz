//! Speculation Fuzzer Main Module
//!
//! This module contains the main entry point and control flow for the speculation fuzzer.

#![allow(incomplete_features)]
#![feature(generic_const_exprs)]
#![feature(stmt_expr_attributes)]
#![feature(proc_macro_hygiene)]
#![no_main]
#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use coverage::page_allocation::PageAllocation;
use custom_processing_unit::{
    apply_hook_patch_func, apply_patch, hook, CustomProcessingUnit, HookGuard,
};
use data_types::addresses::MSRAMHookIndex;
use fuzzer_data::{OtaC2D, OtaC2DTransport, OtaD2CTransport};
use itertools::Itertools;
use log::{error, trace, warn, Level};
#[cfg(feature = "__debug_performance_trace")]
use performance_timing::track_time;
use spec_fuzz::controller_connection::{ConnectionSettings, ControllerConnection};
use spec_fuzz::{check_if_pmc_stable, execute_speculation, patches};
use uefi::boot::ScopedProtocol;
use uefi::proto::loaded_image::LoadedImage;
use uefi::{entry, println, CString16, Status};
use uefi_raw::table::runtime::ResetType;
use uefi_raw::{Ipv4Address, PhysicalAddress};
use uefi_udp4::Ipv4AddressExt;
use x86::cpuid::cpuid;
use x86_perf_counter::PerformanceCounter;

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

/// Main entry point for the speculation fuzzer
///
/// This function initializes the system, sets up the CPU and microcode patches,
/// establishes communication with the controller, and enters the main fuzzing loop.
#[entry]
unsafe fn main() -> Status {
    uefi::helpers::init().unwrap();
    println!("Hello world!");

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

    let allocation = PageAllocation::alloc_address(PhysicalAddress::from(0x1000u64), 1);
    match &allocation {
        Err(err) => {
            error!("Failed to allocate page: {:?}", err);
        }
        Ok(page) => {
            trace!("Page allocated successfully");
            page.ptr().cast::<u16>().write(0x1234);
        }
    }

    let mut cpu = match CustomProcessingUnit::new() {
        Ok(x) => x,
        Err(err) => {
            error!("Failed to create CPU: {:?}", err);
            return Status::ABORTED;
        }
    };

    if let Err(err) = cpu.init() {
        error!("Failed to initialize CPU: {:?}", err);
        return Status::ABORTED;
    }

    if let Err(err) = cpu.zero_hooks() {
        error!("Failed to zero hooks: {:?}", err);
        return Status::ABORTED;
    }

    if let Err(err) = apply_patch(&patches::patch::PATCH) {
        error!("Failed to apply patch: {:?}", err);
        return Status::ABORTED;
    }

    // hook rdrand -> EXPERIMENT
    if let Err(err) = hook(
        apply_hook_patch_func(),
        MSRAMHookIndex::ZERO,
        0x428,
        patches::patch::LABEL_ENTRY,
        true,
    ) {
        error!("Failed to apply hook: {:?}", err);
        return Status::ABORTED;
    }

    // hook rdseed -> SYNCFULL
    if let Err(err) = hook(
        apply_hook_patch_func(),
        MSRAMHookIndex::ZERO + 1,
        0x430,
        patches::patch::LABEL_SYNCFULL,
        true,
    ) {
        error!("Failed to apply hook: {:?}", err);
        return Status::ABORTED;
    }

    let _enable_hooks = HookGuard::disable_all();

    let mut udp: ControllerConnection = {
        trace!("Connecting to UDP");

        match ControllerConnection::connect(&connection_settings) {
            Ok(udp) => udp,
            Err(err) => {
                error!("Failed to connect to controller: {:?}", err);
                return Status::ABORTED;
            }
        }
    };

    if let Err(err) = udp.send(OtaD2CTransport::ResetSession) {
        error!("Failed to send reset-session: {:?}", err);
    }

    if let Err(err) = &allocation {
        let _ = udp.log_reliable(Level::Error, format!("Failed to allocate page: {:?}", err));
    }

    trace!("Waiting for command...");
    #[cfg_attr(
        feature = "__debug_performance_trace",
        track_time("fuzzer_device::main_loop")
    )]
    loop {
        let packet = match udp.receive(None) {
            Ok(None) => continue,
            Ok(Some(packet)) => packet,
            Err(err) => {
                error!("Failed to receive packet: {:?}", err);
                let _ =
                    udp.log_reliable(Level::Error, format!("Failed to receive packet: {:?}", err));
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
                    coverage_collection: false,
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
            OtaC2DTransport::Blacklist { address: _ } => {}
            OtaC2DTransport::DidYouExcludeAnAddressLastRun => {
                let blacklisted = OtaD2CTransport::LastRunBlacklisted { address: None };
                if let Err(err) = udp.send(blacklisted) {
                    error!("Failed to send blacklisted: {:?}", err);
                    let _ = udp.log_reliable(
                        Level::Error,
                        format!("Failed to send blacklisted: {:?}", err),
                    );
                }
                let _ = udp.log_reliable(Level::Error, "Blacklist not supported!".to_string());
            }
            OtaC2DTransport::Reboot => {
                break;
            }
            OtaC2DTransport::AreYouThere => {}
            OtaC2DTransport::GiveMeYourBlacklistedAddresses =>
            #[cfg_attr(
                feature = "__debug_performance_trace",
                track_time("fuzzer_device::main_loop::blacklist-get")
            )]
            {
                let _ = udp.log_reliable(Level::Error, "Blacklist list not supported!".to_string());
            }
            OtaC2DTransport::ReportPerformanceTiming => {
                let _ =
                    udp.log_reliable(Level::Error, "Perf measurement not supported!".to_string());
            }
            OtaC2DTransport::SetRandomSeed { seed: _ } => {
                let _ = udp.log_reliable(Level::Error, "Random seed not supported!".to_string());
            }
            OtaC2DTransport::ExecuteSample { .. } => {
                let _ =
                    udp.log_reliable(Level::Error, "Sample execution not supported!".to_string());
            }
            OtaC2DTransport::TraceSample { .. } => {
                let _ = udp.log_reliable(Level::Error, "Trace sample not supported!".to_string());
            }
            OtaC2DTransport::UCodeSpeculation {
                triad,
                sequence_word,
                perf_counter_setup,
            } => {
                let result =
                    execute_speculation(&mut udp, triad, sequence_word, perf_counter_setup);
                if let Err(err) = udp.send(OtaD2CTransport::UCodeSpeculationResult(result)) {
                    error!("Failed to speculation_x86 results: {:?}", err);
                    let _ = udp.log_reliable(
                        Level::Error,
                        format!("Failed to speculation_x86 results: {:?}", err),
                    );
                }
            }
            OtaC2DTransport::TestIfPMCStable { perf_counter_setup } => {
                let result = check_if_pmc_stable(&mut udp, perf_counter_setup);
                if let Err(err) =
                    udp.send(OtaD2CTransport::PMCStableCheckResults { pmc_stable: result })
                {
                    error!("Failed to send pmc stable check results: {:?}", err);
                    let _ = udp.log_reliable(
                        Level::Error,
                        format!("Failed to send pmc stable check results: {:?}", err),
                    );
                }
            }
            OtaC2DTransport::RunScenario(_name, _payload) => {
                /*info!("Running scenario {}", name);
                let result = poc_agent::execute(&name, payload.as_slice());
                if let Err(err) = udp.send(OtaD2CTransport::ScenarioResult(name, result)) {
                    error!("Failed to send result: {:?}", err);
                }*/
                let _ = udp.log_reliable(Level::Error, "Trace sample not supported!".to_string());
            }
        }
    }

    warn!("Exiting...");
    let _ = udp.log_reliable(Level::Error, format!("Restarting device"));
    uefi::runtime::reset(ResetType::COLD, Status::SUCCESS, None);
}
