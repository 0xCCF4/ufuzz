#![no_main]
#![no_std]
#![allow(named_asm_labels)]
#![allow(unsafe_attr_outside_unsafe)]
extern crate alloc;

use alloc::vec::Vec;
use core::arch;
use core::arch::asm;
use custom_processing_unit::{
    CustomProcessingUnit, HookGuard, StagingBufferAddress, apply_hook_patch_func, apply_patch,
    hook, lmfence, stgbuf_read, stgbuf_write,
};
use data_types::addresses::MSRAMHookIndex;
use log::info;
use speculation::{
    BRANCH_MISSES_RETIRED, INSTRUCTIONS_RETIRED, MS_DECODED_MS_ENTRY, UOPS_ISSUED_ANY,
    UOPS_RETIRED_ANY, patches,
};
use uefi::{Status, entry, print, println};
use x86_perf_counter::PerformanceCounter;

#[entry]
unsafe fn main() -> Status {
    uefi::helpers::init().unwrap();
    print!("Hello world! ");

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

    let disable_hooks = HookGuard::disable_all(); // will be dropped on end of method

    if let Err(err) = apply_patch(&patches::experiment::PATCH) {
        println!("Failed to apply patch: {:?}", err);
        return Status::ABORTED;
    }

    if let Err(err) = hook(
        apply_hook_patch_func(),
        MSRAMHookIndex::ZERO,
        0x428, // rdrand
        patches::experiment::LABEL_EXPERIMENT,
        true,
    ) {
        println!("Failed to apply hook: {:?}", err);
        return Status::ABORTED;
    }

    {
        let enable_hooks = HookGuard::enable_all();
        //test_speculation("Instruction", spec_window);

        spec_ucode();

        enable_hooks.restore();
    }

    disable_hooks.restore();

    Status::SUCCESS
}

fn spec_ucode() -> (u64, u64)  {
    let before: u64;
    let after: u64;
    asm!(
    "mov rcx, rax",
    "rdrand",
    out("rax") after,
    out("rcx") before,
    );
    println!("Before: {:#x} After: {:#x}", before, after);
    (before, after)
}

#[inline(always)]
pub fn flush_decode_stream_buffer() {
    unsafe {
        asm!(
        // Align `ip` to a 64-byte boundary
        "lea {ip}, [rip]",
        "and {ip}, ~63",

        // Copy 512 bytes from `ip` to `ip`
        "mov {index}, 512",
        "mov {ptr}, {ip}",
        "2:",
        "mov {tmp:l}, byte ptr [{ptr}]",
        "mov byte ptr [{ptr}], {tmp:l}",
        "inc {ptr}",
        "dec {index}",
        "jnz 2b",

        // Execute sfence
        "sfence",

        // Flush cache lines
        "mov {index}, 0",
        "3:",
        "clflush byte ptr [{ip} + {index}]",
        "add {index}, 64",
        "cmp {index}, 512",
        "jne 3b",

        ip = out(reg) _,
        index = out(reg) _,
        tmp = out(reg) _,
            ptr = out(reg) _,
        );
    }
}
