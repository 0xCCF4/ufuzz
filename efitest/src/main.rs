#![no_main]
#![no_std]

extern crate alloc;

use core::alloc::Layout;
use core::arch::asm;
use log::info;
use uefi::boot;
use uefi::boot::MemoryType;
use uefi::prelude::*;
use custom_processing_unit::CustomProcessingUnit;

mod patches;

const COUNTER: *mut u64 = 0x10000 as *mut u64;

#[entry]
unsafe fn main() -> Status {
    uefi::helpers::init().unwrap();
    info!("Hello world!");

    *COUNTER = 0;

    info!("Initial random 1: {:?}, {}", rdrand(), *COUNTER);
    boot::stall(2_000_000);

    info!("Initial random 2: {:?}, {}", rdrand(), *COUNTER);

    let cpu = CustomProcessingUnit::new().error_unwrap();
    cpu.init();
    cpu.zero_match_and_patch().error_unwrap();

    info!("After start 0: {:?}, {}", rdrand(), *COUNTER);
    info!("After start 1: {:?}, {}", rdrand(), *COUNTER);
    info!("After start 2: {:?}, {}", rdrand(), *COUNTER);
    info!("After start 3: {:?}, {}", rdrand(), *COUNTER);
    info!("After start 4: {:?}, {}", rdrand(), *COUNTER);

    let patch = crate::patches::rdrand_patch;
    cpu.patch(&patch);
    cpu.hook(0, 0x0428, 0x7c00).error_unwrap();

    info!("After patch 0: {:?}, {}", rdrand(), *COUNTER);
    info!("After patch 1: {:?}, {}", rdrand(), *COUNTER);
    info!("After patch 2: {:?}, {}", rdrand(), *COUNTER);
    info!("After patch 3: {:?}, {}", rdrand(), *COUNTER);
    info!("After patch 4: {:?}, {}", rdrand(), *COUNTER);
    
    cpu.zero_match_and_patch().error_unwrap();
    
    info!("After finish 0: {:?}, {}", rdrand(), *COUNTER);
    info!("After finish 1: {:?}, {}", rdrand(), *COUNTER);
    info!("After finish 2: {:?}, {}", rdrand(), *COUNTER);
    info!("After finish 3: {:?}, {}", rdrand(), *COUNTER);
    info!("After finish 4: {:?}, {}", rdrand(), *COUNTER);

    Status::SUCCESS
}

fn rdrand() -> (u64, bool) {
    let rnd32;
    let flags: u8;
    unsafe {
        asm! {
            "rdrand rax",
            "setc {flags}",
            out("rax") rnd32,
            flags = out(reg_byte) flags,
            options(nomem, nostack)
        }
    }
    (rnd32, flags > 0)
}

trait ErrorUnwrap<T> {
    fn error_unwrap(self) -> T;
}

impl<T, E> ErrorUnwrap<T> for Result<T, E> where E: core::fmt::Display {
    fn error_unwrap(self) -> T {
        match self {
            Err(e) => panic!("Unwrap error: {}", e),
            Ok(content) => content,
        }
    }
}
