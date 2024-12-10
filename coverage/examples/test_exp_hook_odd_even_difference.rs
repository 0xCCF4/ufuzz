#![no_main]
#![no_std]

//! Experiment for testing if one could tell apart if a hook was called by an even or odd address.

use core::ptr::NonNull;
use itertools::Itertools;
use custom_processing_unit::{apply_hook_patch_func, apply_patch, call_custom_ucode_function, hook, CustomProcessingUnit};
use data_types::addresses::{MSRAMHookIndex};
use log::info;
use uefi::{entry, print, println, Status};
use uefi::data_types::PhysicalAddress;
use coverage::page_allocation::PageAllocation;

mod patch {
    use ucode_compiler_derive::patch;

    patch!(
        .org 0x7c00

        <entry>
        include <sys/dump_staging_buffer.up>

        NOPB
        unk_256() !m1 SEQW LFNCEWAIT, UEND0
        NOPB

        <call_even>
        NOP SEQW GOTO 0x428
        NOP
        NOP

        <call_odd>
        NOP SEQW GOTO 0x429
        NOP
        NOP

        NOP
    );
}

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

    apply_patch(&patch::PATCH);

    if let Err(err) = hook(
        apply_hook_patch_func(),
        MSRAMHookIndex::ZERO+0,
        0x428,
        patch::LABEL_ENTRY,
        true,
    ) {
        info!("Failed to hook {:?}", err);
        return Status::ABORTED;
    }

    let size_bytes = 8*0x2000;
    let allocation = match PageAllocation::alloc_address(0x2000 as PhysicalAddress, size_bytes/4096) {
        Ok(allocation) => allocation,
        Err(e) => {
            info!("Failed to allocate memory {:?}", e);
            return Status::ABORTED;
        }
    };
    for i in 0..size_bytes {
        allocation.ptr().add(i).write_volatile(0xf0);
    }
    let ptr: NonNull<u64> = allocation.ptr().cast();

    println!("Starting experiments");

    for chunk in (0usize..0x2000).collect_vec().chunks(1) {
        let first = *chunk.first().unwrap();
        let last = chunk.last().unwrap() + 1;

        print!("\r[{:04x}]->[{:04x}] : ", first, last);
        let result = call_custom_ucode_function(patch::LABEL_CALL_EVEN, [first, last, 0]);
        print!("{:x?}                             ", result);
    }
    println!();

    println!("b800: {:x}", allocation.ptr().add(0xb800).cast::<u64>().read_volatile());
    println!("fff8: {:x}", allocation.ptr().add(0xFFF8).cast::<u64>().read_volatile());

    for (j, c) in (0..0x10).collect_vec().chunks(4).enumerate() {
        if c.iter().all(|v| *v == 0) {
            continue;
        }
        print!("[{:04x}]: ", (j*4) << 3);
        for i in c {
            let data = ptr.add(j * 4 + i).read_volatile();
            print!("{:016x} ", data);
        }
        println!();
    }

    Status::SUCCESS
}

