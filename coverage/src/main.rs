#![no_main]
#![no_std]

extern crate alloc;

use crate::interface::ComInterface;
use crate::page_allocation::PageAllocation;
use crate::patches::patch;
use core::arch::asm;
use custom_processing_unit::{
    apply_patch, call_custom_ucode_function, crbus_read, hook, labels, lmfence, stgbuf_read,
    CustomProcessingUnit,
};
use data_types::addresses::MSRAMHookIndex;
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

        /*
        print!("Time:");
        for i in 0..5 {
            print!("{}, ", interface.read_time_table(i));
        }
        println!();
        */
    }

    apply_patch(&patch);
    /*
    let first_empty_addr = patch.addr + 4*patch.ucode_patch.len();
    // generate patch code
    let mut coverage_entry: [UcodePatchEntry; 1] = patches::coverage_entry::UCODE_PATCH_CONTENT;
    for i in 0..hooks {
        let offset = i * 2;
        let mut entry = &mut coverage_entry[0][1];
        //*entry = (*entry & !(0xFFusize << 24)) | ((offset & 0xFF) << 24);

        apply_patch(&Patch {
            addr: first_empty_addr + i * 4,
            ucode_patch: &coverage_entry,
            hook_address: None,
            hook_index: None,
            labels: &[],
        });
    }
    */*/

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

    //println!("First empty addr: {}", first_empty_addr);
    selfcheck(&mut interface);

    interface.reset_coverage();

    interface.write_jump_table_all(&[0xABCD, 0xDEF0, 0x1111]);

    print_buf();
    println!("RDRAND: {:x?}", rdrand(0));
    read_coverage_table(&interface);

    fn read_hooks() {
        print!("Hooks: ");
        for i in 0..5 {
            let hook = call_custom_ucode_function(patch::LABEL_FUNC_LDAT_READ_HOOKS, [i, 0, 0]).rax;
            print!("{:08x}, ", hook);
        }
        println!();
    }

    read_hooks();

    if let Err(e) = hook(
        patch::LABEL_FUNC_HOOK,
        MSRAMHookIndex::ZERO + 0,
        labels::RDRAND_XLAT,
        patch::LABEL_HOOK_ENTRY_00,
        true,
    ) {
        info!("Failed to hook {:?}", e);
        return Status::ABORTED;
    }

    if let Err(e) = hook(
        patch::LABEL_FUNC_HOOK,
        MSRAMHookIndex::ZERO + 1,
        labels::RDRAND_XLAT,
        patch::LABEL_HOOK_ENTRY_01,
        true,
    ) {
        info!("Failed to hook {:?}", e);
        return Status::ABORTED;
    }

    read_hooks();

    print_buf();
    println!("RDRAND: {:x?}", rdrand(0));
    read_coverage_table(&interface);

    read_hooks();

    println!("RDRAND: {:x?}", rdrand(0));
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

    drop(page);

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
    println!("OK");

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
