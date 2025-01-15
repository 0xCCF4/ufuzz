#![no_main]
#![no_std]

extern crate alloc;

use log::info;
use uefi::runtime::ResetType;
use uefi::{entry, println, Status};

#[entry]
unsafe fn main() -> Status {
    uefi::helpers::init().unwrap();
    info!("Hello world!");

    println!("Goodbye!");
    uefi::runtime::reset(ResetType::SHUTDOWN, Status::SUCCESS, None);
    Status::SUCCESS
}
