#![no_main]
#![no_std]

//! Runtime test for the coverage collector patch (setup) ucode function.

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::arch::asm;
use coverage::harness::coverage_harness::CoverageHarness;
use coverage::interface::safe::ComInterface;
use coverage::{coverage_collector, interface_definition};
use custom_processing_unit::{
    call_custom_ucode_function, lmfence, ms_patch_instruction_read, ms_seqw_read,
    CustomProcessingUnit, FunctionResult, HookGuard,
};
use data_types::addresses::UCInstructionAddress;
use itertools::Itertools;
use log::info;
use ucode_compiler::utils::instruction::Instruction;
use ucode_compiler::utils::sequence_word::SequenceWord;
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

    let addresses = match get_args() {
        Some(addresses) => addresses,
        None => return Status::ABORTED,
    };

    println!("Init harness");
    let mut harness = CoverageHarness::new(&mut interface, &cpu);

    // println!(" ---- NORMAL ---- ");
    // print_status(addresses.len(), &addresses);

    println!(" ---- SETUP ---- ");
    let guard = HookGuard::disable_all();
    if let Err(err) = harness.setup(&addresses) {
        println!("Failed to setup harness: {:?}", err);
        return Status::ABORTED;
    }
    drop(guard);

    print_status(addresses.len(), &addresses);

    println!(" ---- RDRAND test ---- ");

    let result = harness.execute(&addresses, || {
        let a = rdrand();
        let b = rdrand();
        (a.0 && b.0, a.1)
    });

    match result {
        Ok(result) => {
            println!("RDRAND: {:x?}", result.result);
            for entry in result.hooks.iter() {
                println!(
                    " - {}: {}",
                    entry.address(),
                    match entry.coverage() {
                        0 => "NotCovered".to_string(),
                        x => format!("Covered {{ count: {} }}", x),
                    }
                );
            }
        }
        Err(err) => {
            println!("Failed to execute harness: {:?}", err);
            return Status::ABORTED;
        }
    }

    println!("Goodbye!");

    drop(harness);

    // cleanup in reverse order
    cpu.cleanup();

    Status::SUCCESS
}

fn get_args() -> Option<Vec<UCInstructionAddress>> {
    let loaded_image_proto: ScopedProtocol<LoadedImage> =
        match uefi::boot::open_protocol_exclusive(uefi::boot::image_handle()) {
            Err(err) => {
                println!("Failed to open image protocol: {:?}", err);
                return None;
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
            return None;
        }
        Some(options) => String::from_utf8(options).unwrap(),
    };
    println!("Options: {:?}", options);
    let addresses = options
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

    if addresses.len() <= 0 {
        println!("No addresses to hook. Specify args: [HOOK...]");
        return None;
    }

    Some(addresses)
}

fn print_status(max_count: usize, hooks: &[UCInstructionAddress]) {
    print!("Hooks  : ");
    for i in 0..max_count
        .min(interface_definition::COM_INTERFACE_DESCRIPTION.max_number_of_hooks as usize)
    {
        let hook =
            call_custom_ucode_function(coverage_collector::LABEL_FUNC_LDAT_READ_HOOKS, [i, 0, 0])
                .rax;
        print!("{:08x}, ", hook);
    }
    println!();

    for index in 0..4 * max_count
        .min(interface_definition::COM_INTERFACE_DESCRIPTION.max_number_of_hooks as usize)
    {
        print!(
            "EXIT {index:02}_{even_odd}: {address}: ",
            index = index / 4,
            even_odd = (index % 4),
            address = hooks[index / 4].align_even() + (index % 4) / 2
        );
        for offset in 0..3 {
            let instruction = ms_patch_instruction_read(
                coverage_collector::LABEL_FUNC_LDAT_READ,
                coverage_collector::LABEL_HOOK_EXIT_00 + index * 4 + offset,
            );
            let instruction = Instruction::disassemble(instruction as u64);

            let ins_str = match instruction {
                x if x.assemble() == 0x6e75406aa00d => "RESTORE".to_string(),
                x if x.assemble_no_crc() == 0 => "NOP".to_string(),
                x => x.opcode().to_string(),
            };

            print!("{} ", ins_str);
        }

        let seqw = ms_seqw_read(
            coverage_collector::LABEL_FUNC_LDAT_READ,
            coverage_collector::LABEL_HOOK_EXIT_00 + index * 4,
        );
        println!(
            "-> {}",
            SequenceWord::disassemble_no_crc_check(seqw as u32)
                .map_or("ERR".to_string(), |s| s.to_string())
        );
    }
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
