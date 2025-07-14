#![no_main]
#![no_std]

use custom_processing_unit::{
    apply_patch, call_custom_ucode_function, CustomProcessingUnit, HookGuard,
};
use log::info;
use uefi::{entry, println, Status};

mod patch {
    use ucode_compiler_derive::patch;

    patch!(
        .org 0x7c00

        <entry_speculation>
        tmp11 := ZEROEXT_DSZ32(0x1000)
        tmp12 := ZEROEXT_DSZ32(0x1008)
        tmp1 := ZEROEXT_DSZ32(0x1)
        tmp2 := ZEROEXT_DSZ32(0x2)

        tmp0 := SUB_DSZ32(tmp11, tmp1)

        STADPPHYS_DSZ64_ASZ32_SC1(tmp11,, tmp1)
        STADPPHYS_DSZ64_ASZ32_SC1(tmp12,, tmp2) SEQW SYNCFULL

        UJMP(,<skip>)
        <before>

        rbx := ZEROEXT_DSZ32(0xa)
        rax := LDPPHYS_DSZ64_ASZ32_SC1(tmp11)
        UJMP(,<end>)

        <skip>
        UJMPCC_DIRECT_NOTTAKEN_CONDZ(tmp0, <before>)

        <end>
        rax := LDPPHYS_DSZ64_ASZ32_SC1(tmp12)

        NOP SEQW UEND0

        <entry_serialized>

        tmp0 := ZEROEXT_DSZ32(0x1000)
        tmp1 := ZEROEXT_DSZ32(0x1)
        tmp2 := ZEROEXT_DSZ32(0x2)

        STADPPHYS_DSZ64_ASZ32_SC1(tmp0,, tmp1) SEQW SYNCFULL
        NOPB
        rax := LDPPHYS_DSZ64_ASZ32_SC1(tmp0) SEQW SYNCFULL
        NOPB
        STADPPHYS_DSZ64_ASZ32_SC1(tmp0,, tmp2) SEQW SYNCFULL
        NOPB
        rbx := LDPPHYS_DSZ64_ASZ32_SC1(tmp0) SEQW SYNCFULL
        NOPB

        NOP SEQW SYNCFULL, UEND0
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

    let result = core::hint::black_box(call_custom_ucode_function)(
        patch::LABEL_ENTRY_SPECULATION,
        [0, 0, 0],
    );
    println!("{:x?}", result);

    let result =
        core::hint::black_box(call_custom_ucode_function)(patch::LABEL_ENTRY_SERIALIZED, [0, 0, 0]);
    println!("{:x?}", result);

    Status::SUCCESS
}
