#![no_main]
#![no_std]

use core::arch::asm;
use custom_processing_unit::{
    apply_patch, lmfence, CustomProcessingUnit, FunctionResult, HookGuard,
};
use data_types::addresses::Address;
use log::info;
use uefi::{entry, println, Status};

mod patch {
    use ucode_compiler_derive::patch;

    patch!(
        .org 0x7c00

        <entry>
        tmp0 := ZEROEXT_DSZ32(0x1234)
        tmp1 := ZEROEXT_DSZ32(0x5678)
        tmp2 := ZEROEXT_DSZ32(0x9abc)
        tmp3 := ZEROEXT_DSZ32(0xdef1)

        tmp0 := SHL_DSZ64(tmp0, 0x30)
        tmp1 := SHL_DSZ64(tmp1, 0x20)
        tmp2 := SHL_DSZ64(tmp2, 0x10)
        tmp3 := SHL_DSZ64(tmp3, 0x00)

        tmp0 := OR_DSZ64(tmp0, tmp1)
        tmp0 := OR_DSZ64(tmp0, tmp2)
        tmp0 := OR_DSZ64(tmp0, tmp3)

        rax := ZEROEXT_DSZ64(tmp0)
        #rsp := SUB_DSZ64(0x8, rsp)
        #$0e6d00060024
        STADPPHYS_DSZ64_ASZ32_SC1(rsp,,rax)
        #rsp := ADD_DSZ64(0x8, rsp)

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

    // let result = core::hint::black_box(call_custom_ucode_function)(patch::LABEL_ENTRY, [0, 0, 0]);

    let mut data = [0u64; 20];
    let offset = 10;

    let mut result = FunctionResult::default();
    lmfence();
    unsafe {
        asm!(
        "xchg {rbx_tmp}, rbx",
        ".byte 0x0f, 0x0f", // call
        "xchg {rbx_tmp}, rbx",
        // copy rsp+offset to data
        "mov rcx, {count}",
        "mov rsi, rsp",
        "sub rsi, {offset}",
        "mov rdi, {dst}",
        "rep movs qword ptr [rdi], qword ptr [rsi]",
        inout("rax") patch::LABEL_ENTRY.address() => result.rax,
        rbx_tmp = inout(reg) 0usize => result.rbx,
        inout("rcx") 0xd8usize => result.rcx,
        inout("rdx") 0usize => result.rdx,
        count = in(reg) data.len() as u64,
        dst = in(reg) data.as_mut_ptr() as u64,
        out("rdi") _,
        out("rsi") _,
        offset = in(reg) offset*8,
        );
    }
    lmfence();

    println!("Result: {:x?}", result);

    for i in 0..data.len() {
        println!("Data[{}]: {:x?}", i as i64 - offset, data[i]);
    }

    println!("Success");

    Status::SUCCESS
}
