#![no_main]
#![no_std]

extern crate alloc;

use core::arch::asm;
use coverage::coverage_harness::CoverageHarness;
use coverage::interface::safe::ComInterface;
use coverage::{coverage_collector, interface_definition};
use custom_processing_unit::{call_custom_ucode_function, lmfence, ms_patch_instruction_read, ms_seqw_read, CustomProcessingUnit, FunctionResult};
use data_types::addresses::{Address, UCInstructionAddress};
use log::info;
use uefi::prelude::*;
use uefi::{print, println};

#[entry]
unsafe fn main() -> Status {
    uefi::helpers::init().unwrap();
    println!("Hello world!");

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

    let mut harness = CoverageHarness::new(&mut interface, &cpu);
    harness.init();

    print_status();

    let addresses = [
        UCInstructionAddress::from_const(0x428),
        //UCInstructionAddress::from_const(0x42a),
    ];

    if let Err(err) = harness.setup(&addresses) {
        info!("Failed to setup harness: {:?}", err);
    }

    print_status();

    println!("Coverage: {}", harness.get_coverage()[addresses[0].address()]);

    print!("Coverage: ");
    let x = harness.execute(&addresses, |_| {
        rdrand()
    }, ());
    println!("{:x} : {:x?}", harness.get_coverage()[addresses[0].address()], x);

    print!("Coverage: ");
    let x = harness.execute(&addresses, |_| {
        (rdrand(), rdrand())
        //(call_custom_ucode_function(UCInstructionAddress::from_const(0x429), [0,0,0]), call_custom_ucode_function(UCInstructionAddress::from_const(0x429), [0,0,0]))
    }, ());
    println!("{:x} : {:x?}", harness.get_coverage()[addresses[0].address()], x);

    drop(harness);

    if let Err(err) = cpu.zero_hooks() {
        println!("Failed to zero hooks: {:?}", err);
    }

    println!("Goodbye!");

    cpu.cleanup();
    drop(interface);

    println!("Exit!");

    Status::SUCCESS
}

fn print_status() {
    print!("Hooks  : ");
    for i in 0..interface_definition::COM_INTERFACE_DESCRIPTION.max_number_of_hooks as usize {
        let hook =
            call_custom_ucode_function(coverage_collector::LABEL_FUNC_LDAT_READ_HOOKS, [i, 0, 0])
                .rax;
        print!("{:08x}, ", hook);
    }
    println!();

    for i in 0..interface_definition::COM_INTERFACE_DESCRIPTION.max_number_of_hooks as usize {
        print!("EXIT {:02}: ", i);
        let seqw = ms_seqw_read(
            coverage_collector::LABEL_FUNC_LDAT_READ,
            coverage_collector::LABEL_HOOK_EXIT_00 + i * 4,
        );
        print!("{:08x} -> ", seqw);
        for offset in 0..3 {
            let instruction = ms_patch_instruction_read(
                coverage_collector::LABEL_FUNC_LDAT_READ,
                coverage_collector::LABEL_HOOK_EXIT_REPLACEMENT_00 + i * 4 + offset,
            );
            print!("{:012x} ", instruction);
        }
        let seqw = ms_seqw_read(
            coverage_collector::LABEL_FUNC_LDAT_READ,
            coverage_collector::LABEL_HOOK_EXIT_REPLACEMENT_00 + i * 4,
        );
        println!("-> {:08x}", seqw);
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
