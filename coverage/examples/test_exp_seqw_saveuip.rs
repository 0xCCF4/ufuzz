#![no_main]
#![no_std]

//! Experiment for testing of hooking odd addresses.
//!
//! Observations: A hook for an even address is applied. When jumping to the next
//! odd address, the hook redirects the execution, still. But continues to execute the redirect+1 address.
//!
//! Implications: Hooks apply to the hooked address and the next one. One can execute different code when
//! the hook is called by the even or odd address.

use custom_processing_unit::{
    apply_patch, call_custom_ucode_function, CustomProcessingUnit, HookGuard,
};
use log::info;
use uefi::{entry, println, Status};

mod patch {
    use ucode_compiler_derive::patch;

    patch!(
        .org 0x7c00

        <entry>
        NOP
        rax := ZEROEXT_DSZ32(0x1111) SEQW SAVEUIP1, GOTO <redirection>
        NOP

        NOP
        NOP
        NOP

        NOP
        NOP
        NOP

        NOP
        NOP
        NOP

        rax := ZEROEXT_DSZ32(0x5555)
        NOP SEQW LFNCEWAIT, UEND0

        <redirection>

        rbx := ZEROEXT_DSZ32(0x2222)

        tmp13 := READUIP_REGOVR(0x01) !m0
        rcx := ZEROEXT_DSZ32(tmp13) SEQW URET1

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

    println!("Starting experiments");
    let guard = HookGuard::enable_all();
    let result = core::hint::black_box(call_custom_ucode_function)(patch::LABEL_ENTRY, [0, 0, 0]);
    guard.restore();
    println!("{:x?}", result);

    println!("Success");

    Status::SUCCESS
}
