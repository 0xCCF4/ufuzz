#![no_main]
#![no_std]

extern crate alloc;

use crate::interface::ComInterface;
use crate::page_allocation::PageAllocation;
use crate::patches::patch;
use core::arch::asm;
use custom_processing_unit::{apply_patch, call_custom_ucode_function, crbus_read, hook, labels, lmfence, ms_patch_instruction_read, ms_seqw_read, stgbuf_read, CustomProcessingUnit, Error};
use data_types::addresses::{Address, MSRAMHookIndex, MSRAMSequenceWordAddress, UCInstructionAddress};
use log::info;
use ucode_compiler::utils::SequenceWord;
use ucode_compiler::utils::SequenceWordSync::SYNCFULL;
use uefi::prelude::*;
use uefi::{print, println};

mod interface;
mod interface_definition;
mod page_allocation;
mod patches;

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

    let mut interface =
        interface::ComInterface::new(&interface_definition::COM_INTERFACE_DESCRIPTION);
    let hooks = {
        let max_hooks = interface.description.max_number_of_hooks;

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

    interface.reset_coverage();
    // interface.reset_time_table();

    fn get_time() -> usize {
        const CRBUS_CLOCK: usize = 0x2000 | 0x2d7;
        crbus_read(CRBUS_CLOCK)
    }

    #[allow(unused)]
    fn unwrap_clock(value: usize) -> usize {
        (value & 0xffffffffffffffusize) * 0x39 + (value >> 0x37)
    }

    println!("Time: {}", get_time() & 0xFFFF);

    unsafe fn read_coverage_table(interface: &ComInterface) {
        print!("Coverage: ");
        for i in 0..5 {
            print!("{}, ", interface.read_coverage_table(i));
        }
        println!();
    }

    apply_patch(&patch);

    fn read_buf(offset: usize) -> usize {
        stgbuf_read(0xba40 + 0x40 * offset)
    }
    fn print_buf() {
        print!("Buffer: ");
        for i in 0..4 {
            print!("{:08x}, ", read_buf(i));
        }
        println!();
    }

    fn read_hooks() {
        print!("Hooks:    ");
        for i in 0..5 {
            let hook = call_custom_ucode_function(patch::LABEL_FUNC_LDAT_READ_HOOKS, [i, 0, 0]).rax;
            print!("{:08x}, ", hook);
        }
        println!();
    }

    //println!("First empty addr: {}", first_empty_addr);
    selfcheck(&mut interface);
    
    
    unsafe fn hook_address(interface: &mut ComInterface, index: usize, address: UCInstructionAddress) -> core::result::Result<(), Error> {
        interface.write_jump_table(index, address.address() as u16);
        hook(
            patch::LABEL_FUNC_HOOK,
            MSRAMHookIndex::ZERO + index,
            address,
            patch::LABEL_HOOK_ENTRY_00 + index * 4,
            true,
        )
    }
    
    
    interface.reset_coverage();

    println!(" ---- NORMAL ---- ");
    read_hooks();

    println!("RDRAND:  {:x?}", rdrand(0));
    println!("SEQW:     {:x?}", ms_seqw_read(patch::LABEL_FUNC_LDAT_READ, MSRAMSequenceWordAddress::ZERO));
    read_coverage_table(&interface);

    println!(" ---- HOOKING ---- ");
    let _ = hook_address(&mut interface, 0, labels::RDRAND_XLAT);
    read_hooks();

    println!("RDRAND:  {:x?}", rdrand(0));
    println!("SEQW:     {:x?}", ms_seqw_read(patch::LABEL_FUNC_LDAT_READ, MSRAMSequenceWordAddress::ZERO));
    read_coverage_table(&interface);

    println!(" ---- HOOKING ---- ");
    let _ = hook_address(&mut interface, 1, labels::RDRAND_XLAT);
    read_hooks();

    println!("RDRAND:  {:x?}", rdrand(0));
    println!("SEQW:     {:x?}", ms_seqw_read(patch::LABEL_FUNC_LDAT_READ, MSRAMSequenceWordAddress::ZERO));
    read_coverage_table(&interface);

    /*
    let ldat_read = apply_ldat_read_func();
    for i in 0..10 {
        print!("[{}] ", first_empty_addr + i);
        let val = if (i & 3) == 3 {
            ms_seqw_read(ldat_read, first_empty_addr + i)
        } else {
            ms_patch_instruction_read(ldat_read, first_empty_addr + i)
        };
        println!("{:08x}", val);
    }
    */

    /*
    for i in 0..20 {
        if i % 4 == 0 {
            print!("\n[{}] ", UCInstructionAddress::MSRAM_START + i);
        }
        let val = if (i & 3) == 3 {
            ms_seqw_read(patch::LABEL_FUNC_LDAT_READ, UCInstructionAddress::MSRAM_START + i)
        } else {
            ms_patch_instruction_read(patch::LABEL_FUNC_LDAT_READ, UCInstructionAddress::MSRAM_START + i)
        };
        print!("{:012x} ", val);
    }
    println!();*/

    drop(page);

    /*
    println!("Testing sequence words");
    let mut diff = -1;
    for i in 0..0x0100 {
        let seqw = SequenceWord::new().goto(0, i).sync(0, SYNCFULL).assemble();
        let calculation = call_custom_ucode_function(patch::LABEL_CRC_CALC_FUNC, [i, 0, 0]).rax;

        if diff == 0 {
            break;
        } else if diff > 0 {
            diff -= 1;
            println!("{:04x}: {:08x}    {:08x}", i, seqw, calculation);
        } else if seqw != calculation as u32 {
            println!("{:04x}: {:08x} != {:08x}", i, seqw, calculation);
            diff = 5;
        }
    }
    println!("OK");*/

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
