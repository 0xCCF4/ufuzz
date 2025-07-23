#![no_main]
#![no_std]

use core::arch::asm;
use custom_processing_unit::{
    apply_hook_patch_func, apply_patch, hook, CustomProcessingUnit, HookGuard,
};
use data_types::addresses::MSRAMHookIndex;
use log::info;
use uefi::{entry, println, Status};

mod patch {
    use ucode_compiler_derive::patch;

    patch!(
        .org 0x7c00

        <entry>
        NOP
        rax := SHR_DSZ64(rax, 0x20)
        rax := SHL_DSZ64(rax, 0x20)
        rdx := XOR_DSZ64(rdx)
        unk_256() !m1 SEQW SYNCFULL, UEND0

        NOP
        NOP
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
        MSRAMHookIndex::ZERO,
        ucode_dump::dump::cpu_000506CA::RDRAND_XLAT,
        patch::LABEL_ENTRY,
        true,
    ) {
        println!("Failed to apply hook: {:?}", err);
        return Status::ABORTED;
    }

    println!("Starting experiments");
    let guard = HookGuard::enable_all();

    let result_rax: u64;
    let result_rdx: u64;

    unsafe {
        asm!(
            "mov rax, 0x12345678f0000022",
            "mov rax, 0xabcdef12f0000011",
            "rdrand r8",
            out("rax") result_rax,
            out("rdx") result_rdx,
            out("r8") _,
        )
    }

    guard.restore();
    println!("RAX: {:x?}", result_rax);
    println!("RDX: {:x?}", result_rdx);

    println!("Success");

    Status::SUCCESS
}
