#![allow(incomplete_features)]
#![feature(generic_const_exprs)]
#![feature(stmt_expr_attributes)]
#![feature(proc_macro_hygiene)]
#![no_main]
#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::ToString;
use coverage::page_allocation::PageAllocation;
use custom_processing_unit::{
    apply_hook_patch_func, apply_patch, hook, CustomProcessingUnit, HookGuard,
};
use data_types::addresses::MSRAMHookIndex;
use fuzzer_data::{OtaC2D, OtaC2DTransport, OtaD2CTransport};
use log::{error, trace, warn, Level};
#[cfg(feature = "__debug_performance_trace")]
use performance_timing::track_time;
use spec_fuzz::controller_connection::{ConnectionSettings, ControllerConnection};
use spec_fuzz::{check_if_pmc_stable, execute_speculation, patches};
use uefi::{entry, println, Status};
use uefi_raw::table::runtime::ResetType;
use uefi_raw::PhysicalAddress;
use x86::apic::DestinationMode::Physical;
use x86::cpuid::cpuid;
use x86_perf_counter::PerformanceCounter;

#[entry]
unsafe fn main() -> Status {
    // todo setup idt

    uefi::helpers::init().unwrap();
    println!("Hello world!");

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

        match ControllerConnection::connect(&ConnectionSettings::default()) {
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
            OtaC2DTransport::GetCapabilities => {
                let vendor_str = x86::cpuid::CpuId::new()
                    .get_vendor_info()
                    .map(|v| v.to_string())
                    .unwrap_or("---".to_string());
                let processor_version = cpuid!(0x1);
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
            OtaC2DTransport::ExecuteSample { code: _ } => {
                let _ =
                    udp.log_reliable(Level::Error, "Sample execution not supported!".to_string());
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
        }
    }

    warn!("Exiting...");
    let _ = udp.log_reliable(Level::Error, format!("Restarting device"));
    uefi::runtime::reset(ResetType::COLD, Status::SUCCESS, None);
}
