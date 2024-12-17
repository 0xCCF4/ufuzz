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
        NOP SEQW GOTO <entry_even>
        NOP
        NOP

        <entry_odd>
        rbx := ZEROEXT_DSZ32(0x1111)
        NOP SEQW GOTO 0x42a

        <entry_even>
        rax := ZEROEXT_DSZ32(0x2222)
        NOP SEQW GOTO 0x429
        NOP

        <exit>
        rdx := OR_DSZ32(rdx, 0x0044)
        unk_256() !m1 SEQW LFNCEWAIT, UEND0
        NOP

        <experiment_even>
        rdx := ZEROEXT_DSZ32(0x1200)
        NOP
        NOP SEQW GOTO 0x428

        <experiment_odd>
        rdx := ZEROEXT_DSZ32(0x1200)
        NOP
        NOP SEQW GOTO 0x429

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

    apply_patch(&patch::PATCH);

    if let Err(err) = hook(
        apply_hook_patch_func(),
        MSRAMHookIndex::ZERO + 2,
        0x19ca,
        patch::LABEL_EXIT,
        true,
    ) {
        info!("Failed to hook {:?}", err);
        return Status::ABORTED;
    }

    if let Err(err) = hook(
        apply_hook_patch_func(),
        MSRAMHookIndex::ZERO + 0,
        0x428,
        patch::LABEL_ENTRY,
        true,
    ) {
        info!("Failed to hook {:?}", err);
        return Status::ABORTED;
    }

    println!("Starting experiments");
    let guard = HookGuard::enable_all();
    let result =
        core::hint::black_box(call_custom_ucode_function)(patch::LABEL_EXPERIMENT_ODD, [0, 0, 0]);
    guard.restore();
    println!("{:x?}", result);

    let guard = HookGuard::enable_all();
    let result =
        core::hint::black_box(call_custom_ucode_function)(patch::LABEL_EXPERIMENT_EVEN, [0, 0, 0]);
    guard.restore();
    println!("{:x?}", result);

    Status::SUCCESS
}
