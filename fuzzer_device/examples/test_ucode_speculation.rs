#![no_main]
#![no_std]

use coverage::page_allocation::PageAllocation;
use custom_processing_unit::{
    apply_hook_patch_func, apply_patch, hook, hooks_enabled, CustomProcessingUnit, HookGuard,
};
use data_types::addresses::MSRAMHookIndex;
use fuzzer_device::executor::hypervisor::Hypervisor;
use hypervisor::hardware_vt::vmx::{adjust_vmx_control, VmxControl};
use hypervisor::state::{StateDifference, VmState};
use iced_x86::code_asm;
use iced_x86::code_asm::CodeAssembler;
use log::{error, info, warn};
use uefi::{entry, println};
use uefi_raw::{PhysicalAddress, Status};
use x86::current::vmx::vmwrite;
use x86::vmx::vmcs;

mod patches {
    use ucode_compiler_derive::patch;

    patch!(
        .org 7c00

        <experiment>
        rax := ZEROEXT_DSZ64(0x0001)
        rcx := ZEROEXT_DSZ64(0xdede)
        tmp7 := ZEROEXT_DSZ64(0xdddd)

        NOPB
        tmp2 := ZEROEXT_DSZ64(0xabab)
        tmp0 := ZEROEXT_DSZ64(0x1000)
        tmp1 := LDPPHYS_DSZ32_ASZ16_SC1(tmp0)
        tmp0 := SUB_DSZ64(tmp0, tmp1) # change tmp1 -> 0x1010 to remove dependency

        UJMPCC_DIRECT_NOTTAKEN_CONDNZ(tmp0, <taken>)
        <speculative_window>
        $00000c6b26000037 #WRSEGFLD(tmp7, GDT, BASE)
        #-MOVETOCREG_DSZ64(rax, 0x692)
        #-MOVETOCREG_BTS_DSZ64(tmp7, 0x692)
        NOPB

        rdx := ZEROEXT_DSZ64(0xdead) SEQW SYNCFULL, UEND0

        NOPB

        <taken>
        r9 := ZEROEXT_DSZ64(0xabab)
        <syncfull>
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

    let allocation = PageAllocation::alloc_address(PhysicalAddress::from(0x1000u64), 1);
    match &allocation {
        Err(err) => {
            error!("Failed to allocate page: {:?}", err);
        }
        Ok(page) => unsafe {
            page.ptr().cast::<u16>().write(0x1234);
        },
    }

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

    let disable_hooks = HookGuard::enable_all(); // will be dropped on end of method

    if let Err(err) = apply_patch(&patches::PATCH) {
        println!("Failed to apply patch: {:?}", err);
        return Status::ABORTED;
    }

    if let Err(err) = hook(
        apply_hook_patch_func(),
        MSRAMHookIndex::ZERO,
        ucode_dump::dump::cpu_000506CA::RDRAND_XLAT,
        patches::LABEL_EXPERIMENT,
        true,
    ) {
        println!("Failed to apply hook: {:?}", err);
        return Status::ABORTED;
    }

    if let Err(err) = hook(
        apply_hook_patch_func(),
        MSRAMHookIndex::ZERO + 1,
        ucode_dump::dump::cpu_000506CA::RDSEED_XLAT,
        patches::LABEL_SYNCFULL,
        true,
    ) {
        println!("Failed to apply hook: {:?}", err);
        return Status::ABORTED;
    }

    let mut code = match CodeAssembler::new(64) {
        Ok(assembler) => assembler,
        Err(e) => {
            info!("Failed to create code assembler {:?}", e);
            return Status::ABORTED;
        }
    };

    if let Err(err) = code.rdseed(code_asm::r10) {
        info!("Failed to assemble rdrand instruction: {:?}", err);
        return Status::ABORTED;
    }
    if let Err(err) = code.rdrand(code_asm::r11) {
        info!("Failed to assemble rdrand instruction: {:?}", err);
        return Status::ABORTED;
    }
    if let Err(err) = code.rdseed(code_asm::r12) {
        info!("Failed to assemble rdseed instruction: {:?}", err);
        return Status::ABORTED;
    }

    let code = match code.assemble(0) {
        Ok(code) => code,
        Err(e) => {
            info!("Failed to assemble code: {:?}", e);
            return Status::ABORTED;
        }
    };

    let mut hypervisor = match Hypervisor::new() {
        Ok(hypervisor) => hypervisor,
        Err(e) => {
            info!("Failed to initiate hypervisor {:?}", e);
            return Status::ABORTED;
        }
    };

    let mut state_before = VmState::default();
    let mut state_after = VmState::default();

    hypervisor.prepare_vm_state();
    hypervisor.capture_state(&mut state_before);
    hypervisor.load_code_blob(&code);

    if let Err(err) = vmwrite(
        vmcs::control::SECONDARY_PROCBASED_EXEC_CONTROLS,
        match adjust_vmx_control(
            VmxControl::ProcessorBased2,
            (vmcs::control::SecondaryControls::ENABLE_EPT
                | vmcs::control::SecondaryControls::UNRESTRICTED_GUEST)
                .bits() as u64,
        ) {
            Ok(x) => x,
            Err(value) => {
                if (value as u32 & vmcs::control::SecondaryControls::ENABLE_EPT.bits()) == 0 {
                    error!("Failed to adjust SECONDARY_PROCBASED_EXEC_CONTROLS. Enable EPT.",);
                    return Status::ABORTED;
                } else if (value as u32
                    & vmcs::control::SecondaryControls::UNRESTRICTED_GUEST.bits())
                    == 0
                {
                    error!("Failed to adjust SECONDARY_PROCBASED_EXEC_CONTROLS. Enable unrestricted guest.");
                    return Status::ABORTED;
                } else {
                    value
                }
            }
        },
    ) {
        error!("Failed to create VMCS hypervisor adjustment: {:?}", err);
    }

    println!("Hooking before: {}", hooks_enabled());
    let exit = hypervisor.run_vm();
    println!("Hooking after: {}", hooks_enabled());
    hypervisor.capture_state(&mut state_after);

    println!("Exit reason: {:#?}", exit);
    println!("State differences:");

    for (reg, before, after) in state_before.difference(&state_after) {
        println!(
            "  Register {}: Before = {:x?}, After = {:x?}",
            reg, before, after
        );
    }

    println!("Finished....");

    Status::SUCCESS
}
