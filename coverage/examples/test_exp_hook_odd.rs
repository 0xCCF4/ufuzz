#![no_main]
#![no_std]

use custom_processing_unit::{apply_hook_patch_func, apply_patch, call_custom_ucode_function, enable_hooks, hook, CustomProcessingUnit};
use data_types::addresses::{MSRAMHookIndex, UCInstructionAddress};
use log::info;
use uefi::{entry, println, Status};
use data_types::addresses::Address;

mod patch {
    use ucode_compiler_derive::patch;

    patch!(
        .org 0x7c00

        <entry>
        rax := ZEROEXT_DSZ32(0x5555)
        rcx := ZEROEXT_DSZ32(0xfefe)
        unk_256() !m1 SEQW LFNCEWAIT, UEND0

        <failsafe>
        rax := ZEROEXT_DSZ32(0x2222)
        unk_256() !m1 SEQW LFNCEWAIT, UEND0
        NOP

        <failsafe_2>
        rax := ZEROEXT_DSZ32(0x1111)
        unk_256() !m1 SEQW LFNCEWAIT, UEND0
        NOP

        <experiment>
        rax := ZEROEXT_DSZ32(0x3333)
        rbx := ZEROEXT_DSZ32(0x1) SEQW GOTO 0x429
        NOP

        NOP
    );

    // U0428: tmp4:= ZEROEXT_DSZ32(0x0000002b)
    // U0429: tmp2:= ZEROEXT_DSZ32(0x40004e00)
    // U042a: tmp0:= ZEROEXT_DSZ32(0x00000439) SEQW GOTO U19c9
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

    apply_patch(&patch::PATCH);

    enable_hooks();

    if let Err(err) = hook(
        apply_hook_patch_func(),
        MSRAMHookIndex::ZERO+2,
        UCInstructionAddress::from_const(0x19ca),
        patch::LABEL_FAILSAFE,
        true,
    ) {
        info!("Failed to hook {:?}", err);
        return Status::ABORTED;
    }
    if let Err(err) = hook(
        apply_hook_patch_func(),
        MSRAMHookIndex::ZERO+3,
        UCInstructionAddress::from_const(0x42c),
        patch::LABEL_FAILSAFE_2,
        true,
    ) {
        info!("Failed to hook {:?}", err);
        return Status::ABORTED;
    }

    println!("Starting experiments");

    // sanity check
    // just call the function
    assert_eq!(0x5555, experiment(0, patch::LABEL_ENTRY.address()));
    assert_eq!(0x2222, experiment(0, patch::LABEL_FAILSAFE.address()));
    assert_eq!(0x1111, experiment(0, patch::LABEL_FAILSAFE_2.address()));

    // some more sanity checks
    // hook something, then run into the hook
    assert_eq!(0x2222, experiment(0x000, 0x428));
    assert_eq!(0x5555, experiment(0x428, 0x428));
    assert_eq!(0x5555, experiment(0x42a, 0x428));
    assert_eq!(0x5555, experiment(0x42a, 0x429));
    assert_eq!(0x5555, experiment(0x42a, 0x42a));
    assert_eq!(0x1111, experiment(0x42a, 0x42b));

    // now hook and run the odd address
    if let Err(err) = hook(
        apply_hook_patch_func(),
        MSRAMHookIndex::ZERO,
        UCInstructionAddress::from_const(0x428),
        patch::LABEL_ENTRY,
        true,
    ) {
        info!("Failed to hook {:?}", err);
        return Status::ABORTED;
    }
    println!("{:x}", core::hint::black_box(call_custom_ucode_function)(UCInstructionAddress::from_const(0x429), [0, 0, 0]).rax);


    println!("Success");

    Status::SUCCESS
}

fn experiment(hook_address: usize, call_address: usize) -> usize {
    let hook_address = UCInstructionAddress::from_const(hook_address);
    let call_address = UCInstructionAddress::from_const(call_address);

    if let Err(err) = hook(
        apply_hook_patch_func(),
        MSRAMHookIndex::ZERO,
        hook_address,
        patch::LABEL_ENTRY,
        true,
    ) {
        info!("Failed to hook {:?}", err);
        return 0;
    }

    let x = call_custom_ucode_function(call_address, [0, 0, 0]);

    x.rax
}
