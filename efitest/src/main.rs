#![no_main]
#![no_std]

extern crate alloc;

use core::arch::asm;
use core::ptr::NonNull;
use log::info;
use uefi::boot::{AllocateType, MemoryType};
use uefi::data_types::PhysicalAddress;
use uefi::prelude::*;
use custom_processing_unit::{stgbuf_read, stgbuf_write, CustomProcessingUnit};

mod patches;

struct PageAllocation {
    base: NonNull<u8>,
    count: usize,
}

impl Drop for PageAllocation {
    fn drop(&mut self) {
        unsafe { uefi::boot::free_pages(self.base, self.count).error_unwrap() }
    }
}

impl PageAllocation {
    pub fn alloc_address(address: PhysicalAddress, count: usize) -> uefi::Result<PageAllocation> {
        let data = uefi::boot::allocate_pages(AllocateType::Address(address), MemoryType::LOADER_DATA, count)?;
        Ok(PageAllocation {
            count,
            base: data
        })
    }
    pub fn ptr(&self) -> &NonNull<u8> {
        &self.base
    }
}

impl AsRef<NonNull<u8>> for PageAllocation {
    fn as_ref(&self) -> &NonNull<u8> {
        &self.base
    }
}

#[allow(dead_code)]
unsafe fn random_counter() {
    let allocation = PageAllocation::alloc_address(0x10000, 1).error_unwrap();
    info!("Allocated: {:?}", allocation.ptr());

    const COUNTER: *mut u64 = 0x10000 as *mut u64;
    *COUNTER = 0;

    info!("Random 1: {:?}, {}", rdrand(), *COUNTER);
    info!("Random 2: {:?}, {}", rdrand(), *COUNTER);

    info!("Patching...");

    let cpu = CustomProcessingUnit::new().error_unwrap();
    cpu.init();
    cpu.zero_match_and_patch().error_unwrap();

    info!("Random 3: {:?}, {}", rdrand(), *COUNTER);
    info!("Random 4: {:?}, {}", rdrand(), *COUNTER);
    info!("Random 5: {:?}, {}", rdrand(), *COUNTER);
    info!("Random 6: {:?}, {}", rdrand(), *COUNTER);
    info!("Random 7: {:?}, {}", rdrand(), *COUNTER);

    info!("Hooking...");

    let patch = crate::patches::rdrand_patch;
    cpu.patch(&patch);
    cpu.hook(0, 0x0428, 0x7c00).error_unwrap();

    info!("Random 8: {:?}, {}", rdrand(), *COUNTER);
    info!("Random 9: {:?}, {}", rdrand(), *COUNTER);
    info!("Random 10: {:?}, {}", rdrand(), *COUNTER);
    info!("Random 11: {:?}, {}", rdrand(), *COUNTER);
    info!("Random 12: {:?}, {}", rdrand(), *COUNTER);

    info!("Restoring...");

    cpu.zero_match_and_patch().error_unwrap();

    info!("Random 13: {:?}, {}", rdrand(), *COUNTER);
    info!("Random 14: {:?}, {}", rdrand(), *COUNTER);
    info!("Random 15: {:?}, {}", rdrand(), *COUNTER);
    info!("Random 16: {:?}, {}", rdrand(), *COUNTER);
    info!("Random 17: {:?}, {}", rdrand(), *COUNTER);

    drop(allocation)
}

unsafe fn random_coverage() {
    fn read_buf() -> usize {
        stgbuf_read(0xba00)
    }
    fn write_buf(val: usize) {
        stgbuf_write(0xba00, val)
    }

    info!("Random 1: {:?}, {}", rdrand(), read_buf());
    write_buf(0);
    info!("Random 2: {:?}, {}", rdrand(), read_buf());

    let cpu = CustomProcessingUnit::new().error_unwrap();
    cpu.init();
    cpu.zero_match_and_patch().error_unwrap();

    let patch = crate::patches::coverage_handler;
    cpu.patch(&patch);
    cpu.hook(0, 0x0428, patch.addr).error_unwrap();

    info!("Random 3: {:?}, {}", rdrand(), read_buf());
    info!("Random 4: {:?}, {}", rdrand(), read_buf());
    info!("Random 5: {:?}, {}", rdrand(), read_buf());

    cpu.hook(0, 0x0428, patch.addr).error_unwrap();

    info!("Re-enabled coverage hook");

    info!("Random 6: {:?}, {}", rdrand(), read_buf());
    info!("Random 7: {:?}, {}", rdrand(), read_buf());
    info!("Random 8: {:?}, {}", rdrand(), read_buf());

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
