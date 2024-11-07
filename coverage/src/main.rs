#![no_main]
#![no_std]

extern crate alloc;

use core::arch::asm;
use log::info;
use uefi::prelude::*;
use custom_processing_unit::CustomProcessingUnit;
use crate::error_unwrap::ErrorUnwrap;

mod page_allocation;
mod patches;
mod error_unwrap;


#[entry]
unsafe fn main() -> Status {
    uefi::helpers::init().unwrap();
    info!("Hello world!");

    let cpu = match CustomProcessingUnit::new() {
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

    let hooks = {
        let max_hooks = 64;

        let device_max_hooks = match cpu.current_glm_version {
            custom_processing_unit::GLM_OLD => 63,
            custom_processing_unit::GLM_NEW => 64,
            _ => 0,
        };

        max_hooks.min(device_max_hooks)
    };

    if hooks == 0 {
        info!("No hooks available");
        return Status::ABORTED;
    }



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
