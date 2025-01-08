#![no_main]
#![no_std]

extern crate alloc;

use alloc::string::String;
use core::arch::asm;
use coverage::harness::coverage_harness::CoverageHarness;
use coverage::interface::safe::ComInterface;
use coverage::{coverage_collector, coverage_collector_debug_tools, interface_definition};
use custom_processing_unit::{
    apply_patch, call_custom_ucode_function, lmfence, ms_seqw_read, CustomProcessingUnit,
    FunctionResult,
};
use data_types::addresses::{Address, UCInstructionAddress};
use itertools::Itertools;
use log::info;
use uefi::boot::ScopedProtocol;
use uefi::prelude::*;
use uefi::proto::loaded_image::LoadedImage;
use uefi::{print, println};

#[entry]
unsafe fn main() -> Status {
    uefi::helpers::init().unwrap();
    info!("Hello world!");

    let mut cpu = match CustomProcessingUnit::new() {
        Ok(cpu) => cpu,
        Err(e) => {
            info!("Failed to initiate program {:?}", e);
            return Status::ABORTED;
        }
    };
    if let Err(e) = cpu.init() {
        info!("Failed to initiate program {:?}", e);
        return Status::ABORTED;
    }

    if let Err(e) = cpu.zero_hooks() {
        info!("Failed to zero hooks {:?}", e);
        return Status::ABORTED;
    }

    let mut interface = match ComInterface::new(&interface_definition::COM_INTERFACE_DESCRIPTION) {
        Ok(interface) => interface,
        Err(e) => {
            info!("Failed to initiate program {:?}", e);
            return Status::ABORTED;
        }
    };
    let hooks = {
        let max_hooks = interface.description().max_number_of_hooks;

        let device_max_hooks = match cpu.current_glm_version {
            custom_processing_unit::GLM_OLD => 31,
            custom_processing_unit::GLM_NEW => 32,
            _ => 0,
        };

        max_hooks.min(device_max_hooks)
    };

    if hooks == 0 {
        info!("No hooks available");
        return Status::ABORTED;
    }

    interface.reset_coverage();

    fn read_hooks() {
        print!("Hooks:    ");
        for i in 0..8 {
            let hook = call_custom_ucode_function(
                coverage_collector_debug_tools::LABEL_FUNC_LDAT_READ_HOOKS,
                [i, 0, 0],
            )
            .rax;
            print!("{:08x}, ", hook);
        }
        println!();
    }

    fn read_seqws() {
        print!("SEQW:     ");
        for i in 0..8 {
            let seqw = ms_seqw_read(
                coverage_collector_debug_tools::LABEL_FUNC_LDAT_READ,
                coverage_collector::LABEL_HOOK_EXIT_00 + i * 4,
            );
            print!("{:08x}, ", seqw);
        }
        println!();
    }

    let mut harness = match CoverageHarness::new(&mut interface, &cpu) {
        Ok(harness) => harness,
        Err(e) => {
            info!("Failed to initiate harness {:?}", e);
            return Status::ABORTED;
        }
    };

    if coverage_collector::PATCH.addr + coverage_collector::PATCH.ucode_patch.len() * 4
        >= coverage_collector_debug_tools::PATCH.addr
    {
        println!("Patch too large. Cannot continue.");
        return Status::ABORTED;
    }
    if let Err(err) = apply_patch(&coverage_collector_debug_tools::PATCH) {
        println!("Failed to apply patch: {:?}", err);
        return Status::ABORTED;
    }

    let loaded_image_proto: ScopedProtocol<LoadedImage> =
        match uefi::boot::open_protocol_exclusive(uefi::boot::image_handle()) {
            Err(err) => {
                println!("Failed to open image protocol: {:?}", err);
                return Status::ABORTED;
            }
            Ok(loaded_image_proto) => loaded_image_proto,
        };
    let options = match loaded_image_proto.load_options_as_bytes().map(|options| {
        options
            .into_iter()
            .filter(|p| **p != 0 && (p.is_ascii_alphanumeric() || p.is_ascii_whitespace()))
            .cloned()
            .collect_vec()
    }) {
        None => {
            println!("No args set.");
            return Status::ABORTED;
        }
        Some(options) => String::from_utf8(options).unwrap(),
    };
    println!("Options: {:?}", options);
    let mut addresses = options
        .split(" ")
        .map(|address| {
            let address = address.trim();
            if address.len() == 0 {
                return None;
            }
            match usize::from_str_radix(address, 16) {
                Err(_err) => None,
                Ok(address) => {
                    if address < 0x7c00 {
                        Some(UCInstructionAddress::from(address))
                    } else {
                        None
                    }
                }
            }
        })
        .filter(|address| address.is_some())
        .map(|address| address.unwrap())
        .collect_vec();

    if addresses.len() <= 1 {
        println!("No addresses to hook. Specify args: [number of executions] [HOOK...]");
        return Status::SUCCESS;
    }

    let number_of_executions = addresses[0].address();
    addresses.remove(0);

    println!("Hooking: {:?}", addresses);

    println!(" ---- Execute {number_of_executions} times ----");
    match harness.execute(&addresses, || {
        read_hooks();
        let x = rdrand();
        println!("RDRAND: {:?}", x);
        for _ in 0..(number_of_executions - 1) {
            let x = rdrand();
            if !x.0 {
                println!("RDRAND: {:?}", x);
                break;
            }
        }
    }) {
        Err(err) => println!("Failed to execute experiment: {:?}", err),
        Ok(x) => {
            print!(
                "{} ",
                x.hooks.iter().map(|entry| entry.coverage()).join(",")
            );
            println!("\n{:x?}", x.result);
            for entry in x.hooks {
                println!(" - {:?}", entry);
            }
        }
    }
    read_hooks();
    read_seqws();

    println!("Goodbye!");
    Status::SUCCESS
}

fn rdrand() -> (bool, FunctionResult) {
    let mut result = FunctionResult::default();
    let flags: u8;
    lmfence();
    unsafe {
        asm! {
        "xchg {rbx_tmp}, rbx",
        "rdrand rax",
        "setc {flags}",
        "xchg {rbx_tmp}, rbx",
        inout("rax") 0usize => result.rax,
        rbx_tmp = inout(reg) 0usize => result.rbx,
        inout("rcx") 0usize => result.rcx,
        inout("rdx") 0usize => result.rdx,
        flags = out(reg_byte) flags,
        options(nostack),
        }
    }
    lmfence();
    (flags > 0, result)
}
