#![no_main]
#![no_std]

use custom_processing_unit::{
    apply_hook_patch_func, apply_patch, call_custom_ucode_function, hook, CustomProcessingUnit,
    HookGuard,
};
use data_types::addresses::MSRAMHookIndex;
use log::info;
use uefi::{entry, println, Status};

mod patch {
    use ucode_compiler_derive::patch;

    patch!(
        .org 0x7c00

        <entry>
        UJMP(,<entry0>)
        UJMP(,<entry1>)
        UJMP(,<entry2>)

        <entry0>
        rdx := MOVEFROMCREG_DSZ64(,0x68) !m2
        rax := ZEROEXT_DSZ32(0x1000) SEQW LFNCEWAIT, UEND0
        NOPB

        <entry1>
        rdx := MOVEFROMCREG_DSZ64(,0x68) !m2
        rax := ZEROEXT_DSZ32(0x2000) SEQW LFNCEWAIT, UEND0
        NOPB

        <entry2>
        rdx := MOVEFROMCREG_DSZ64(,0x68) !m2
        rax := ZEROEXT_DSZ32(0x3000) SEQW LFNCEWAIT, UEND0
        NOPB

        <experiment>
        UJMP(,0x7be0)
        UJMP(,0x7be1)
        UJMP(,0x7be2)

        rdx := MOVEFROMCREG_DSZ64(,0x68) !m2
        rax := ZEROEXT_DSZ32(0x4000) SEQW LFNCEWAIT, UEND0
        NOPB

        <exit_failsafe>
        rdx := MOVEFROMCREG_DSZ64(,0x68) !m2
        rax := ZEROEXT_DSZ32(0x5000) SEQW LFNCEWAIT, UEND0
        NOPB


        NOP
    );
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

    if let Err(err) = apply_patch(&patch::PATCH) {
        println!("Failed to apply patch: {:?}", err);
        return Status::ABORTED;
    }

    if let Err(err) = hook(
        apply_hook_patch_func(),
        MSRAMHookIndex::ZERO + 0,
        0x7be0,
        patch::LABEL_ENTRY,
        true,
    ) {
        info!("Failed to hook {:?}", err);
        return Status::ABORTED;
    }
    if let Err(err) = hook(
        apply_hook_patch_func(),
        MSRAMHookIndex::ZERO + 1,
        0x7be4,
        patch::LABEL_EXIT_FAILSAFE,
        true,
    ) {
        info!("Failed to hook {:?}", err);
        return Status::ABORTED;
    }

    println!("Starting experiments");
    let guard = HookGuard::enable_all();
    let result =
        core::hint::black_box(call_custom_ucode_function)(patch::LABEL_EXPERIMENT + 0, [0, 0, 0]);
    guard.restore();
    println!("{:x?}", result);

    let guard = HookGuard::enable_all();
    let result =
        core::hint::black_box(call_custom_ucode_function)(patch::LABEL_EXPERIMENT + 1, [0, 0, 0]);
    guard.restore();
    println!("{:x?}", result);

    let guard = HookGuard::enable_all();
    let result =
        core::hint::black_box(call_custom_ucode_function)(patch::LABEL_EXPERIMENT + 2, [0, 0, 0]);
    guard.restore();
    println!("{:x?}", result);

    if let Err(err) = hook(
        apply_hook_patch_func(),
        MSRAMHookIndex::ZERO + 0,
        0x7be0 + 2,
        patch::LABEL_ENTRY,
        true,
    ) {
        info!("Failed to hook {:?}", err);
        return Status::ABORTED;
    }

    let guard = HookGuard::enable_all();
    let result =
        core::hint::black_box(call_custom_ucode_function)(patch::LABEL_EXPERIMENT + 0, [0, 0, 0]);
    guard.restore();
    println!("{:x?}", result);

    let guard = HookGuard::enable_all();
    let result =
        core::hint::black_box(call_custom_ucode_function)(patch::LABEL_EXPERIMENT + 1, [0, 0, 0]);
    guard.restore();
    println!("{:x?}", result);

    let guard = HookGuard::enable_all();
    let result =
        core::hint::black_box(call_custom_ucode_function)(patch::LABEL_EXPERIMENT + 2, [0, 0, 0]);
    guard.restore();
    println!("{:x?}", result);

    Status::SUCCESS
}
