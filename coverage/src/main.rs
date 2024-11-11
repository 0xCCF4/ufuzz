#![no_main]
#![no_std]

extern crate alloc;

use crate::patches::patch;
use core::arch::asm;
use custom_processing_unit::{
    apply_patch, hook, labels, ms_hook_read,
    stgbuf_read, stgbuf_write_raw, CustomProcessingUnit,
};
use data_types::addresses::{MSRAMHookIndex};
use log::info;
use uefi::prelude::*;
use uefi::{print, println};

mod page_allocation;
mod patches;

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

    fn read_buf() -> usize {
        stgbuf_read(0xba00)
    }
    fn write_buf(val: usize) {
        stgbuf_write_raw(0xba00, val)
    }

    info!("Random 1: {:?}, {}", rdrand(), read_buf());
    write_buf(0);
    info!("Random 2: {:?}, {}", rdrand(), read_buf());

    apply_patch(&patch);

    if let Err(err) = hook(
        patch::LABEL_FUNC_HOOK,
        MSRAMHookIndex::ZERO,
        labels::RDRAND_XLAT,
        patch.addr,
        true,
    ) {
        info!("Failed to hook {:?}", err);
        return Status::ABORTED;
    }
    info!("Hooked coverage hook");

    info!("Random 3: {:?}, {}", rdrand(), read_buf());
    info!("Random 4: {:?}, {}", rdrand(), read_buf());
    info!("Random 5: {:?}, {}", rdrand(), read_buf());

    if let Err(err) = hook(
        patch::LABEL_FUNC_HOOK,
        MSRAMHookIndex::ZERO,
        labels::RDRAND_XLAT,
        patch::LABEL_FAKE_RND,
        true,
    ) {
        info!("Failed to hook {:?}", err);
        return Status::ABORTED;
    }

    info!("Re-enabled coverage hook");

    info!("Random 6: {:?}, {}", rdrand(), read_buf());
    info!("Random 7: {:?}, {}", rdrand(), read_buf());
    info!("Random 8: {:?}, {}", rdrand(), read_buf());

    let _ = cpu.zero_hooks();

    for i in 0..32 {
        if i > 31 {
            break;
        }
        print!("[{i:04x}] ");
        let val = ms_hook_read(patch::LABEL_FUNC_LDAT_READ, MSRAMHookIndex::ZERO + i);
        println!("{val:013x}");
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
