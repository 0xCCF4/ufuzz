#![no_main]
#![no_std]

extern crate alloc;

use core::arch::asm;
use custom_processing_unit::{CustomProcessingUnit, apply_hook_patch_func};
use data_types::addresses::MSRAMHookIndex;
use log::info;
use uefi::prelude::*;
use uefi::{print, println};

mod patch {
    use ucode_compiler_derive::patch;

    patch!(
        .org 0x7c00

        <poc>
        rax := ZEROEXT_DSZ64(0xcafe)
    );
}

#[entry]
unsafe fn main() -> Status {
    uefi::helpers::init().unwrap();
    println!("Hello world!");
    
    // Preparing execution

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
    
    // Start of experiment

    print!("\nGenerating some random numbers: ");
    for _ in 0..4 {
        print!("{}", rdrand().1);
    }
    println!("\nYou should see, several random numbers");

    println!("\n=== ENABLING MICROCODE HOOK ===\n");

    if let Err(err) = custom_processing_unit::hook(
        apply_hook_patch_func(),
        MSRAMHookIndex::ZERO,
        ucode_dump::dump::cpu_000506CA::RDRAND_XLAT,
        patch::LABEL_POC,
        true,
    ) {
        println!("Failed to apply patch {:?}", err);
    }

    print!("Generating some random numbers: ");
    for _ in 0..4 {
        print!("{:x}", rdrand().1);
    }
    println!("You should, see several times the same random number");
    
    // End of experiment

    if let Err(err) = cpu.zero_hooks() {
        println!("Failed to zero hooks: {:?}", err);
    }

    println!("Exit!");

    Status::SUCCESS
}

fn rdrand() -> (bool, u64) {
    let flags: u8;
    let result: u64;
    unsafe {
        asm! {
        "rdrand rax",
        "setc {flags}",
        flags = out(reg_byte) flags,
        out("rax") result,
        options(nostack),
        }
    }
    (flags > 0, result)
}
