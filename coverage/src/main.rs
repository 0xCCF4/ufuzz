#![no_main]
#![no_std]

extern crate alloc;

use crate::patches::patch;
use core::arch::asm;
use core::ptr::NonNull;
use custom_processing_unit::{apply_patch, call_custom_ucode_function, hook, labels, lmfence, CustomProcessingUnit};
use log::info;
use uefi::prelude::*;
use uefi::{print, println};
use data_types::addresses::MSRAMHookIndex;
use crate::interface::ComInterface;
use crate::page_allocation::PageAllocation;

mod page_allocation;
mod patches;
mod interface_definition;
mod interface;

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

    let hooks = {
        let max_hooks = 32;

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

    let page = match PageAllocation::alloc_address(0x1000, 1) {
        Ok(page) => page,
        Err(e) => {
            info!("Failed to allocate page {:?}", e);
            return Status::ABORTED;
        }
    };

    let mut interface = interface::ComInterface::new(&interface_definition::COM_INTERFACE_DESCRIPTION);
    interface.reset_coverage();

    unsafe fn read_coverage_table(interface: &ComInterface) {
        print!("Coverage: ");
        for i in 0..5 {
            print!("{}, ", interface.read_coverage_table(i));
        }
        println!();
    }

    apply_patch(&patch);
    selfcheck(&mut interface);

    interface.reset_coverage();

    println!("RDRAND: {:?}", rdrand(0));
    read_coverage_table(&interface);

    if let Err(e) = hook(patch::LABEL_FUNC_HOOK, MSRAMHookIndex::ZERO+1, labels::RDRAND_XLAT, patch::LABEL_ENTRY_RDRAND_XLAT, true) {
        info!("Failed to hook {:?}", e);
        return Status::ABORTED;
    }

    println!("RDRAND: {:?}", rdrand(0));
    read_coverage_table(&interface);

    println!("RDRAND: {:?}", rdrand(0));
    read_coverage_table(&interface);

    drop(page);

    Status::SUCCESS
}

unsafe fn selfcheck(interface: &mut ComInterface) -> bool {
    for i in 0..interface.description.max_number_of_hooks {
        interface.write_jump_table(i, i as u16);
    }
    // check
    interface.reset_coverage();
    for i in 0..interface.description.max_number_of_hooks {
        let val = interface.read_coverage_table(i);
        if val != 0 {
            println!("Coverage table not zeroed at index {}", i);
            return false;
        }
    }

    let result = call_custom_ucode_function(patch::LABEL_SELFCHECK_FUNC, [0, 0, 0]);
    if result.rax != 0xABC || result.rbx >> 1 != interface.description.max_number_of_hooks {
        println!("Selfcheck invoke failed: {:x?}", result);
        return false;
    }

    for i in 0..interface.description.max_number_of_hooks {
        let val = interface.read_coverage_table(i);
        if val != i as u16 {
            println!("Selfcheck mismatch [{i:x}] {val:x}");
            return false;
        }
    }

    true
}

fn rdrand(index: u8) -> (u64, bool) {
    let rnd32;
    let flags: u8;
    lmfence();
    unsafe {
        asm! {
        "rdrand rax",
        "setc {flags}",
        inout("rax") index as u64 => rnd32,
        flags = out(reg_byte) flags,
        options(nomem, nostack)
        }
    }
    lmfence();
    (rnd32, flags > 0)
}
