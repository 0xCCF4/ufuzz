#![no_main]
#![no_std]

extern crate alloc;

use core::arch::asm;
use coverage::interface::safe::ComInterface;
use coverage::{coverage_collector, interface_definition};
use custom_processing_unit::{
    apply_patch, call_custom_ucode_function, labels, lmfence, ms_seqw_read,
    CustomProcessingUnit, FunctionResult,
};
use data_types::addresses::{
    MSRAMSequenceWordAddress, UCInstructionAddress,
};
use log::info;
use uefi::prelude::*;
use uefi::{print, println};

#[entry]
unsafe fn main() -> Status {
    uefi::helpers::init().unwrap();
    info!("Hello world!");

    let cpu = match CustomProcessingUnit::new() {
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

    unsafe fn read_coverage_table(interface: &ComInterface) {
        print!("Coverage: ");
        for i in 0..8 {
            print!("{}, ", interface.read_coverage_table(i));
        }
        println!();
    }

    apply_patch(&coverage_collector::PATCH);

    fn read_hooks() {
        print!("Hooks:    ");
        for i in 0..8 {
            let hook = call_custom_ucode_function(
                coverage_collector::LABEL_FUNC_LDAT_READ_HOOKS,
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
                coverage_collector::LABEL_FUNC_LDAT_READ,
                coverage_collector::LABEL_HOOK_EXIT_00 + i*4
            );
            print!("{:08x}, ", seqw);
        }
        println!();
    }

    unsafe fn setup_hooks(interface: &mut ComInterface, addresses: &[UCInstructionAddress]) {
        interface.write_jump_table_all(addresses);

        let result = call_custom_ucode_function(
            coverage_collector::LABEL_FUNC_SETUP,
            [addresses.len(), 0, 0],
        );

        println!("Setup: {:x?}", result);

        if result.rax != 0x664200006642 {
            println!("Failed to setup");
        }
    }

    read_hooks();

    apply_patch(&coverage_collector::PATCH); // todo: remove

    interface.reset_coverage();

    println!(" ---- NORMAL ---- ");
    read_hooks();

    println!("RDRAND:  {:x?}", rdrand());
    read_seqws();
    read_coverage_table(&interface);

    println!(" ---- HOOKING ---- ");
    let addr = [
        labels::RDRAND_XLAT,
        labels::RDRAND_XLAT + 2,
        labels::RDRAND_XLAT + 4,
    ];
    setup_hooks(&mut interface, &addr);
    read_hooks();
    read_seqws();
    read_coverage_table(&interface);

    for _i in 0..2 {
        println!(" ---- RUN ---- ");
        println!("RDRAND:  {:x?}", rdrand());
        read_seqws();
        read_hooks();
        read_coverage_table(&interface);
    }

    println!(" ---- WAITING ---- ");
    boot::stall(2e6 as usize);
    read_hooks();
    read_coverage_table(&interface);

    if let Err(err) = cpu.zero_hooks() {
        println!("Failed to zero hooks: {:?}", err);
    }

    println!("Goodbye!");

    // cleanup in reverse order
    cpu.cleanup();

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
