#![no_main]
#![no_std]

//! Experiment for testing of hooking odd addresses.
//!
//! Observations: A hook for an even address is applied. When jumping to the next
//! odd address, the hook redirects the execution, still. But continues to execute the redirect+1 address.
//!
//! Implications: Hooks apply to the hooked address and the next one. One can execute different code when
//! the hook is called by the even or odd address.

use custom_processing_unit::{apply_hook_patch_func, apply_patch, call_custom_ucode_function, hook, CustomProcessingUnit, HookGuard};
use data_types::addresses::{MSRAMHookIndex};
use log::info;
use uefi::{entry, println, Status};

mod patch {
    use ucode_compiler_derive::patch;

    patch!(
        .org 0x7c00

        <entry0>
        rax := ZEROEXT_DSZ32(0x5555)
        rcx := ZEROEXT_DSZ32(0xfefe)
        NOP SEQW GOTO 0x42a

        <entry1>
        rax := ZEROEXT_DSZ32(0x5555)
        rcx := ZEROEXT_DSZ32(0xfefe)
        NOP SEQW GOTO 0x9c2

        <exit>
        rdx := OR_DSZ32(rdx, 0x0022)
        unk_256() !m1 SEQW LFNCEWAIT, UEND0
        NOP

        <experiment0>
        rdx := ZEROEXT_DSZ32(0x1200)
        NOP
        NOP SEQW GOTO 0x429

        <experiment0_even>
        rdx := ZEROEXT_DSZ32(0x1200)
        NOP
        NOP SEQW GOTO 0x428

        <experiment1>
        rdx := ZEROEXT_DSZ32(0x1200)
        NOP
        NOP SEQW GOTO 0x9c1

        NOP
    );

    // Experiment 1:
    // HOOK <------------  U0428: tmp4:= ZEROEXT_DSZ32(0x0000002b)
    //  |       CALL --->  U0429: tmp2:= ZEROEXT_DSZ32(0x40004e00)
    //  |             /->  U042a: tmp0:= ZEROEXT_DSZ32(0x00000439) SEQW GOTO U19c9
    //  |             |
    //  \-> rcx := fefe    HOOKED:
    //                     U19ca: rax := 0x2222
    //                     U19cb: RETURN

    // Experiment 2:
    // HOOK <------------  U09c0: tmp0:= ZEROEXT_DSZ32(0x00000001)
    //  |       CALL --->  U09c1: tmp2:= ZEROEXT_DSZ32N(IMM_MACRO_02) !m0,m1
    //  |             /->  U09c2: tmp1:= SUB_DSZN(0x00000001, rcx) !m1
    //  |             |
    //  \-> rcx := fefe    HOOKED:
    //                     U09c4: rax := 0x2222
    //                     U09c5: RETURN
}

#[entry]
unsafe fn main() -> Status {
    uefi::helpers::init().unwrap();
    println!("Hello world!");

    let mut cpu = match CustomProcessingUnit::new() {
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

    let _guard = HookGuard::disable_all(); // will be dropped on end of method

    apply_patch(&patch::PATCH);

    if let Err(err) = hook(
        apply_hook_patch_func(),
        MSRAMHookIndex::ZERO+2,
        0x19ca,
        patch::LABEL_EXIT,
        true,
    ) {
        info!("Failed to hook {:?}", err);
        return Status::ABORTED;
    }
    if let Err(err) = hook(
        apply_hook_patch_func(),
        MSRAMHookIndex::ZERO+3,
        0x9c4,
        patch::LABEL_EXIT,
        true,
    ) {
        info!("Failed to hook {:?}", err);
        return Status::ABORTED;
    }

    if let Err(err) = hook(
        apply_hook_patch_func(),
        MSRAMHookIndex::ZERO+0,
        0x428,
        patch::LABEL_ENTRY0,
        true,
    ) {
        info!("Failed to hook {:?}", err);
        return Status::ABORTED;
    }

    if let Err(err) = hook(
        apply_hook_patch_func(),
        MSRAMHookIndex::ZERO+1,
        0x9c0,
        patch::LABEL_ENTRY1,
        true,
    ) {
        info!("Failed to hook {:?}", err);
        return Status::ABORTED;
    }

    println!("Starting experiments");
    let guard = HookGuard::enable_all();
    let result = core::hint::black_box(call_custom_ucode_function)(patch::LABEL_EXPERIMENT0, [0, 0, 0]);
    guard.restore();
    println!("{:x?}", result);

    let guard = HookGuard::enable_all();
    let result = core::hint::black_box(call_custom_ucode_function)(patch::LABEL_EXPERIMENT0_EVEN, [0, 0, 0]);
    guard.restore();
    println!("{:x?}", result);

    let guard = HookGuard::enable_all();
    let result = core::hint::black_box(call_custom_ucode_function)(patch::LABEL_EXPERIMENT1, [0, 0, 0]);
    guard.restore();
    println!("{:x?}", result);
    println!("Success");

    Status::SUCCESS
}

