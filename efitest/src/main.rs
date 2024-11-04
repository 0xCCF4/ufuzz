#![no_main]
#![no_std]

extern crate alloc;

use crate::helpers::PageAllocation;
use core::arch::asm;
use custom_processing_unit::{
    labels, ms_patch_ram_read, ms_patch_ram_write, stgbuf_read, stgbuf_write_raw,
    CustomProcessingUnit,
};
use data_types::addresses::{LinearAddress, MSRAMHookAddress};
use log::info;
use uefi::boot::ScopedProtocol;
use uefi::prelude::*;
use uefi::proto::console::text::Input;
use uefi::{print, println};

mod helpers;
mod patches;

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
    cpu.hook(MSRAMHookAddress::ZERO, labels::RDRAND_XLAT, patch.addr)
        .error_unwrap();

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

#[allow(dead_code)]
unsafe fn random_coverage() {
    fn read_buf() -> usize {
        stgbuf_read(0xba00)
    }
    fn write_buf(val: usize) {
        stgbuf_write_raw(0xba00, val)
    }

    info!("Random 1: {:?}, {}", rdrand(), read_buf());
    write_buf(0);
    info!("Random 2: {:?}, {}", rdrand(), read_buf());

    let cpu = CustomProcessingUnit::new().error_unwrap();
    cpu.init();
    cpu.zero_match_and_patch().error_unwrap();

    let patch = crate::patches::coverage_handler;
    cpu.patch(&patch);
    cpu.hook(MSRAMHookAddress::ZERO, labels::RDRAND_XLAT, patch.addr)
        .error_unwrap();

    info!("Random 3: {:?}, {}", rdrand(), read_buf());
    info!("Random 4: {:?}, {}", rdrand(), read_buf());
    info!("Random 5: {:?}, {}", rdrand(), read_buf());

    cpu.hook(MSRAMHookAddress::ZERO, labels::RDRAND_XLAT, patch.addr)
        .error_unwrap();

    info!("Re-enabled coverage hook");

    info!("Random 6: {:?}, {}", rdrand(), read_buf());
    info!("Random 7: {:?}, {}", rdrand(), read_buf());
    info!("Random 8: {:?}, {}", rdrand(), read_buf());

    cpu.zero_match_and_patch().error_unwrap();
}

fn ldat_read() {
    let ldat_read = crate::patches::ldat_read;
    let cpu = CustomProcessingUnit::new().error_unwrap();
    cpu.init();
    cpu.zero_match_and_patch().error_unwrap();

    /*
    cpu.patch(&ldat_read);

    let rdrand_patch = crate::patches::rdrand_patch;
    let mut rdrand_patch_read_before = [[0usize; 4]; 8];
    assert_eq!(rdrand_patch.ucode_patch.len(), rdrand_patch_read_before.len());
    read_patch(ldat_read.addr, rdrand_patch.addr, &mut rdrand_patch_read_before);
    cpu.patch(&rdrand_patch);
    let mut rdrand_patch_read = [[0usize; 4]; 8];
    assert_eq!(rdrand_patch.ucode_patch.len(), rdrand_patch_read.len());
    read_patch(ldat_read.addr, rdrand_patch.addr, &mut rdrand_patch_read);

    println!(" ADR. | SHOULD_BE   | READ_VALUE  | BEFORE");
    for (i, ((should_be, read_value), before_value)) in rdrand_patch.ucode_patch.iter().zip(rdrand_patch_read.iter()).zip(rdrand_patch_read_before.iter()).enumerate() {
        let address = rdrand_patch.addr + i*4;

        for offset in 0..3 {
            let address = address + offset;

            let different = if should_be[offset] == read_value[offset] {
                ""
            } else {
                "<"
            };
            println!("[{:04x}] {:013x} {:013x} {:013x} {}", address, should_be[offset], read_value[offset], before_value[offset], different);
        }
    }
    */

    for i in 0..128 * 3 {
        print!("[{i:04x}] Writing... ");
        ms_patch_ram_write(LinearAddress::from_const(i), i);
        println!("OK");
    }

    cpu.patch(&ldat_read);

    for i in 0..128 * 3 {
        print!("[{i:04x}] ");
        let val = ms_patch_ram_read(ldat_read.addr, LinearAddress::from_const(i));
        let difference = if val != i { "<" } else { "" };
        println!("{val:013x} {difference}");

        if i % 10 == 0 {
            wait_for_key_press();
        }
    }
}

#[entry]
unsafe fn main() -> Status {
    uefi::helpers::init().unwrap();
    info!("Hello world!");

    info!("Random counter test");
    //random_counter();
    //wait_for_key_press();

    info!("Random coverage test");
    //random_coverage();
    //wait_for_key_press();

    info!("Check patch integrity");
    ldat_read();

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

fn wait_for_key_press() {
    let handle = uefi::boot::get_handle_for_protocol::<Input>().error_unwrap();
    let mut keyboard: ScopedProtocol<Input> =
        uefi::boot::open_protocol_exclusive(handle).error_unwrap();

    loop {
        let event = keyboard.wait_for_key_event().error_unwrap();
        uefi::boot::wait_for_event(&mut [event]).error_unwrap();
        if keyboard.read_key().error_unwrap().is_some() {
            return;
        }
    }
}

trait ErrorUnwrap<T> {
    fn error_unwrap(self) -> T;
}

impl<T, E> ErrorUnwrap<T> for Result<T, E>
where
    E: core::fmt::Display,
{
    #[track_caller]
    fn error_unwrap(self) -> T {
        match self {
            Err(e) => panic!("Result unwrap error: {}", e),
            Ok(content) => content,
        }
    }
}

impl<T> ErrorUnwrap<T> for Option<T> {
    #[track_caller]
    fn error_unwrap(self) -> T {
        match self {
            None => panic!("Option unwrap error: None"),
            Some(content) => content,
        }
    }
}
