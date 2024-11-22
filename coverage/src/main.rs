#![no_main]
#![no_std]

extern crate alloc;

use coverage::page_allocation::PageAllocation;
use core::arch::asm;
use coverage::interface::safe::ComInterface;
use coverage::{coverage_collector, interface, interface_definition};
use custom_processing_unit::{apply_patch, calculate_hook_value, call_custom_ucode_function, hook, labels, lmfence, ms_seqw_read, CustomProcessingUnit, FunctionResult};
use data_types::addresses::{
    Address, MSRAMHookIndex, MSRAMSequenceWordAddress, UCInstructionAddress,
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

    //println!("First empty addr: {}", first_empty_addr);
    selfcheck(&mut interface);

    unsafe fn hook_address(
        interface: &mut ComInterface,
        index: usize,
        address: UCInstructionAddress,
    ) {
        interface.write_jump_table(index, address.address() as u16);

        if let Err(err) = hook(
            coverage_collector::LABEL_FUNC_HOOK,
            MSRAMHookIndex::ZERO + index,
            address,
            coverage_collector::LABEL_HOOK_ENTRY_00 + index * 4,
            true,
        ) {
            println!("Failed to hook: {:?}", err);
        }
    }

    unsafe fn hook_many(interface: &mut ComInterface, addresses: &[UCInstructionAddress]) {
        interface.zero_jump_table();
        interface.write_jump_table_all(addresses);

        /*let result =  call_custom_ucode_function(coverage_collector::LABEL_FUNC_HOOK_MANY, [0, 0, 0]);
        println!("Hook Many: {:x?}", result);
        if result.rax != 0x334100003341 {
            println!("Failed to hook many");
        }*/
    }

    read_hooks();
    /*hook_many(&mut interface, &[
        labels::RDRAND_XLAT,
        labels::RDRAND_XLAT+2,
        labels::RDRAND_XLAT+4,
        UCInstructionAddress::from_const(0x1000),
        UCInstructionAddress::from_const(0x2000),
        UCInstructionAddress::from_const(0x3000),
        UCInstructionAddress::from_const(0x4000),
    ]);*/
    read_hooks();
    let _ = cpu.zero_hooks();

    interface.reset_coverage();

    println!(" ---- NORMAL ---- ");
    read_hooks();

    println!("RDRAND:  {:x?}", rdrand(0));
    println!("SEQW:     {:x?}", ms_seqw_read(coverage_collector::LABEL_FUNC_LDAT_READ, MSRAMSequenceWordAddress::ZERO));
    read_coverage_table(&interface);

    println!(" ---- HOOKING ---- ");
    for i in 0..interface.description().max_number_of_hooks {
        hook_address(&mut interface, i, labels::RDRAND_XLAT + i * 2);
    }
    read_hooks();
    println!("SEQW:     {:x?}", ms_seqw_read(coverage_collector::LABEL_FUNC_LDAT_READ, MSRAMSequenceWordAddress::ZERO));
    read_coverage_table(&interface);

    for _i in 0..2 {
        println!(" ---- RUN ---- ");
        println!("RDRAND:  {:x?}", rdrand(0));
        println!(
            "SEQW:     {:x?}",
            ms_seqw_read(
                coverage_collector::LABEL_FUNC_LDAT_READ,
                MSRAMSequenceWordAddress::ZERO
            )
        );
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

unsafe fn selfcheck(interface: &mut ComInterface) -> bool {
    for i in 0..interface.description().max_number_of_hooks {
        interface.write_jump_table(i, i as u16);
    }
    // check
    interface.reset_coverage();
    for i in 0..interface.description().max_number_of_hooks {
        let val = interface.read_coverage_table(i);
        if val != 0 {
            println!("Coverage table not zeroed at index {}", i);
            return false;
        }
    }

    let result = call_custom_ucode_function(coverage_collector::LABEL_SELFCHECK_FUNC, [0, 0, 0]);
    if result.rax != 0xABC || result.rbx >> 1 != interface.description().max_number_of_hooks {
        println!("Selfcheck invoke failed: {:x?}", result);
        return false;
    }

    for i in 0..interface.description().max_number_of_hooks {
        let val = interface.read_coverage_table(i);
        if val != i as u16 {
            println!("Selfcheck mismatch [{i:x}] {val:x}");
            return false;
        }
    }

    true
}

fn rdrand(index: u8) -> (bool, FunctionResult) {
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
