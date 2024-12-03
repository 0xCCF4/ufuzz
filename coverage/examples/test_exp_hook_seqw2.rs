#![no_main]
#![no_std]

use core::arch::asm;
use custom_processing_unit::{apply_hook_patch_func, apply_patch, call_custom_ucode_function, hook, lmfence, CustomProcessingUnit};
use log::info;
use uefi::{entry, print, println, Status};
use data_types::addresses::{MSRAMHookIndex, UCInstructionAddress};

mod patch {
    use ucode_compiler_derive::patch;

    patch!(
        .org 0x7c00

        <func_ldat_read_hooks>
        include <sys/func_ldat_read_hooks.up>

        <entry>
        rbx := ZEROEXT_DSZ32(0xabab)

        # disables hook entry 0
        r10 := ZEROEXT_DSZ64(0x0)
        r11 := ZEROEXT_DSZ64(0x0)

        #func lib/pause_frontend(r12, r11)
        #func lib/ldat_write(r10, r11, r11, 3)
        #func lib/resume_frontend(r12)

        tmp4:= ZEROEXT_DSZ32(0x0000002b) SEQW GOTO 0x19c9

        NOPB

        <exit>
        #rax := ZEROEXT_DSZ32(0x7777)
        #rbx := ZEROEXT_DSZ32(0x7777)
        rcx := ZEROEXT_DSZ32(0x7777)
        rdx := ZEROEXT_DSZ32(0x7777)
    );

    // U06bc: rax:= ZEROEXT_DSZ8(tmp1, rax) SEQW UEND0
    // U06bd: NOP
    // U06be: UJMP( , tmp3)
}

#[entry]
unsafe fn main() -> Status {
    uefi::helpers::init().unwrap();

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

    if let Err(e) = cpu.zero_hooks() {
        info!("Failed to zero hooks {:?}", e);
        return Status::ABORTED;
    }

    apply_patch(&patch::PATCH);
    let patch_func = apply_hook_patch_func();

    if let Err(err) = hook(patch_func, MSRAMHookIndex::ZERO, UCInstructionAddress::from_const(0x42a), patch::LABEL_ENTRY, true) {
        info!("Failed to hook {:?}", err);
        return Status::ABORTED;
    }
    if let Err(err) = hook(patch_func, MSRAMHookIndex::ZERO+1, UCInstructionAddress::from_const(0x19ca), patch::LABEL_EXIT, true) {
        info!("Failed to hook {:?}", err);
        return Status::ABORTED;
    }

    #[derive(Debug, Copy, Clone, Default)]
    struct Result {
        rax: usize,
        rbx: usize,
        rcx: usize,
        rdx: usize,
        r10: usize,
        r11: usize,
        r12: usize,
        flags: u8,
    }
    let mut result = Result::default();

    println!("Before: {:x?}", result);
    read_hooks();

    lmfence();
    unsafe {
        asm!(
        "xchg {rbx_tmp}, rbx",
        "rdrand rax",
        "setc {flags}",
        "xchg {rbx_tmp}, rbx",
        inout("rax") 0usize => result.rax,
        rbx_tmp = inout(reg) 0usize => result.rbx,
        inout("rcx") 0usize => result.rcx,
        inout("rdx") 0usize => result.rdx,
        inout("r10") 0usize => result.r10,
        inout("r11") 0usize => result.r11,
        inout("r12") 0usize => result.r12,
        flags = out(reg_byte) result.flags,
        options(nostack)
        );
    }
    lmfence();

    println!("After: {:x?}", result);
    read_hooks();

    Status::SUCCESS
}

fn read_hooks() {
    print!("Hooks:    ");
    for i in 0..8 {
        let hook = call_custom_ucode_function(
            patch::LABEL_FUNC_LDAT_READ_HOOKS,
            [i, 0, 0],
        )
            .rax;
        print!("{:08x}, ", hook);
    }
    println!();
}