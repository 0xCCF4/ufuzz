#![no_main]
#![no_std]
#![allow(incomplete_features)]
#![feature(generic_const_exprs)]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt::Debug;
use fuzzer_device::cmos;
use fuzzer_device::cmos::CMOS;
use performance_timing::track_time;
use uefi::boot::ScopedProtocol;
use uefi::proto::loaded_image::LoadedImage;
use uefi::{entry, print, println, Status};

#[entry]
unsafe fn main() -> Status {
    uefi::helpers::init().unwrap();
    println!("Hello world!");

    if let Err(err) = performance_timing::initialize(1.09e9) {
        println!("Failed to initialize performance timing: {:?}", err);
        return Status::ABORTED;
    }

    println!("Starting timing measurements...");

    for i in (1..10).rev() {
        print!("{i:02}\r");
        uefi::boot::stall(1_000_000);
    }

    for i in 0..10 {
        sleep();
    }

    println!(
        "{}",
        performance_timing::measurements::mm_instance().borrow()
    );

    uefi::boot::stall(20_000_000);

    Status::SUCCESS
}

#[track_time]
fn sleep() {
    uefi::boot::stall(1_000_000);
}
