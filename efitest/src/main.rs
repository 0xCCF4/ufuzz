#![no_main]
#![no_std]

extern crate alloc;

use core::arch::asm;
use log::info;
use uefi::prelude::*;
use custom_processing_unit::{stgbuf_read, stgbuf_write, CustomProcessingUnit};

mod patches;

#[allow(dead_code)]
unsafe fn random_counter() {
    const COUNTER: *mut u64 = 0x10000 as *mut u64;

    *COUNTER = 0;

    info!("Initial random 1: {:?}, {}", rdrand(), *COUNTER);
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
}

unsafe fn random_coverage() {
    fn read_buf() -> usize {
        stgbuf_read(0xba00)
    }
    fn write_buf(val: usize) {
        stgbuf_write(0xba00, val)
    }

    info!("Initial random 1: {:?}, {}", rdrand(), read_buf());
    write_buf(0);
    info!("Initial random 2: {:?}, {}", rdrand(), read_buf());

    let cpu = CustomProcessingUnit::new().error_unwrap();
    cpu.init();
    cpu.zero_match_and_patch().error_unwrap();

    let patch = crate::patches::coverage_handler;
    cpu.patch(&patch);
    cpu.hook(0, 0x0428, patch.addr).error_unwrap();

    info!("Patched random 1: {:?}, {}", rdrand(), read_buf());
    info!("Patched random 2: {:?}, {}", rdrand(), read_buf());
    info!("Patched random 3: {:?}, {}", rdrand(), read_buf());

    cpu.hook(0, 0x0428, patch.addr).error_unwrap();

    info!("Re-enabled coverage hook");

    info!("Patched random 4: {:?}, {}", rdrand(), read_buf());
    info!("Patched random 5: {:?}, {}", rdrand(), read_buf());
    info!("Patched random 6: {:?}, {}", rdrand(), read_buf());

    cpu.zero_match_and_patch().error_unwrap();
}

#[entry]
unsafe fn main() -> Status {
    uefi::helpers::init().unwrap();
    info!("Hello world!");

    random_coverage();

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
