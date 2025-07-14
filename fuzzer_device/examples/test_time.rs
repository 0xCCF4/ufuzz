#![no_main]
#![no_std]
#![allow(incomplete_features)]
#![feature(generic_const_exprs)]

extern crate alloc;

use performance_timing::measurements::MeasurementCollection;
use performance_timing::track_time;
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

    for _ in 0..10 {
        sleep();
    }

    println!(
        "{}",
        MeasurementCollection::from(
            performance_timing::measurements::mm_instance()
                .borrow()
                .data
                .clone()
        )
        .normalize()
    );

    uefi::boot::stall(20_000_000);

    Status::SUCCESS
}

#[track_time]
fn sleep() {
    uefi::boot::stall(1_000_000);
}
