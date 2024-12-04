#![no_main]
#![no_std]

//! Experiment for testing what happens when two hooks are applied to the same triad.
//!
//! Observations: The hook for the index 2 is applied, but the hook for the index 0 is discarded.
//! The declaration order of the hooks does not matter.
//!
//! Implications: Hook only one address per triad.

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

        <entry_0428>
        rbx := ZEROEXT_DSZ32(0x0428)
        tmp4:= ZEROEXT_DSZ32(0x0000002b) SEQW GOTO 0x0429

        <entry_042a>
        rcx := ZEROEXT_DSZ32(0x042a)
        tmp0:= ZEROEXT_DSZ32(0x00000439) SEQW GOTO 0x19c9
        NOPB

        <entry_19ca>
        rdx := ZEROEXT_DSZ32(0x19ca)
    );

    // U06bc: rax:= ZEROEXT_DSZ8(tmp1, rax) SEQW UEND0
    // U06bd: NOP
    // U06be: UJMP( , tmp3)

    // U0428: tmp4:= ZEROEXT_DSZ32(0x0000002b)
    // U0429: tmp2:= ZEROEXT_DSZ32(0x40004e00)
    // U042a: tmp0:= ZEROEXT_DSZ32(0x00000439) SEQW GOTO U19c9

    // U19c8: FETCHFROMEIP1_ASZ64( , tmp6) !m1 SEQW UEND0
    // U19c9: tmp1:= READURAM( , 0x0035, 64)
    // U19ca: TESTUSTATE( , SYS, UST_SMM) !m1 ? SEQW GOTO U19ce
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

    // this was also tested with different orderings and indexes

    if let Err(err) = hook(patch_func, MSRAMHookIndex::ZERO+0, UCInstructionAddress::from_const(0x42a), patch::LABEL_ENTRY_042A, true) {
        info!("Failed to hook {:?}", err);
        return Status::ABORTED;
    }
    if let Err(err) = hook(patch_func, MSRAMHookIndex::ZERO+1, UCInstructionAddress::from_const(0x428), patch::LABEL_ENTRY_042A, true) {
        info!("Failed to hook {:?}", err);
        return Status::ABORTED;
    }
    if let Err(err) = hook(patch_func, MSRAMHookIndex::ZERO+2, UCInstructionAddress::from_const(0x19ca), patch::LABEL_ENTRY_19CA, true) {
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