//! # Speculation Ucode
//!
//! An experiment for analyzing and testing CPU microcode-level speculation behavior.

#![no_main]
#![no_std]
#![allow(named_asm_labels)]
#![allow(unsafe_attr_outside_unsafe)]
extern crate alloc;

use core::arch::asm;
use core::mem;
use core::mem::offset_of;
use coverage::page_allocation::PageAllocation;
use custom_processing_unit::{
    CustomProcessingUnit, HookGuard, apply_hook_patch_func, apply_patch, hook, hooks_enabled,
};
use data_types::addresses::MSRAMHookIndex;
use hypervisor::state::{GuestRegisters, StateDifference};
use log::{error, info, trace};
use speculation_ucode::patches;
use uefi::data_types::PhysicalAddress;
use uefi::{Status, entry, print, println};

#[entry]
unsafe fn main() -> Status {
    uefi::helpers::init().unwrap();
    println!("Hello world! ");

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

    if let Err(err) = apply_patch(&patches::patch::PATCH) {
        println!("Failed to apply patch: {:?}", err);
        return Status::ABORTED;
    }

    if let Err(err) = hook(
        apply_hook_patch_func(),
        MSRAMHookIndex::ZERO,
        ucode_dump::dump::cpu_000506CA::RDRAND_XLAT,
        patches::patch::LABEL_EXPERIMENT,
        true,
    ) {
        println!("Failed to apply hook: {:?}", err);
        return Status::ABORTED;
    }

    if let Err(err) = hook(
        apply_hook_patch_func(),
        MSRAMHookIndex::ZERO + 1,
        ucode_dump::dump::cpu_000506CA::RDSEED_XLAT,
        patches::patch::LABEL_SYNCFULL,
        true,
    ) {
        println!("Failed to apply hook: {:?}", err);
        return Status::ABORTED;
    }

    {
        flush_decode_stream_buffer();
        spec_ucode();
    }

    disable_hooks.restore();

    Status::SUCCESS
}

#[inline(always)]
fn capture_state(state: &mut GuestRegisters) {
    unsafe {
        asm!(
            "mov [{state}+{offset_rax}], rax",
            "mov [{state}+{offset_rbx}], rbx",
            "mov [{state}+{offset_rcx}], rcx",
            "mov [{state}+{offset_rdx}], rdx",
            "mov [{state}+{offset_rsi}], rsi",
            "mov [{state}+{offset_rdi}], rdi",
            "mov [{state}+{offset_r8}], r8",
            "mov [{state}+{offset_r9}], r9",
            "mov [{state}+{offset_r10}], r10",
            "mov [{state}+{offset_r11}], r11",
            "mov [{state}+{offset_r12}], r12",
            "mov [{state}+{offset_r13}], r13",
            "mov [{state}+{offset_r14}], r14",
            "mov [{state}+{offset_r15}], r15",
            "mov [{state}+{offset_rsp}], rsp",

            "push rax",
            "pushfq",
            "pop rax",
            "mov [{state}+{offset_rflags}], rax",
            "pop rax",

            state = in(reg) state,
            offset_rax = const offset_of!(GuestRegisters, rax),
            offset_rbx = const offset_of!(GuestRegisters, rbx),
            offset_rcx = const offset_of!(GuestRegisters, rcx),
            offset_rdx = const offset_of!(GuestRegisters, rdx),
            offset_rsi = const offset_of!(GuestRegisters, rsi),
            offset_rdi = const offset_of!(GuestRegisters, rdi),
            offset_r8 = const offset_of!(GuestRegisters, r8),
            offset_r9 = const offset_of!(GuestRegisters, r9),
            offset_r10 = const offset_of!(GuestRegisters, r10),
            offset_r11 = const offset_of!(GuestRegisters, r11),
            offset_r12 = const offset_of!(GuestRegisters, r12),
            offset_r13 = const offset_of!(GuestRegisters, r13),
            offset_r14 = const offset_of!(GuestRegisters, r14),
            offset_r15 = const offset_of!(GuestRegisters, r15),
            offset_rsp = const offset_of!(GuestRegisters, rsp),
            offset_rflags = const offset_of!(GuestRegisters, rflags),
        );
    }
}

fn spec_ucode() {
    let mut before = GuestRegisters::default();
    let mut after = GuestRegisters::default();

    println!("Hooks enabled before: {}", hooks_enabled());

    unsafe {
        asm!(
        // save values for later reference
        "pushfq",
        "push rax",
        "push rbx",
        "push rcx",
        "push rdx",
        "push rsi",
        "push rdi",
        "push r8",
        "push r9",
        "push r10",
        "push r11",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        "push rbp",
        "sub rsp, 0x100",
        "movaps xmmword ptr [rsp], xmm0",
        "movaps xmmword ptr [rsp + 0x10], xmm1",
        "movaps xmmword ptr [rsp + 0x20], xmm2",
        "movaps xmmword ptr [rsp + 0x30], xmm3",
        "movaps xmmword ptr [rsp + 0x40], xmm4",
        "movaps xmmword ptr [rsp + 0x50], xmm5",
        "movaps xmmword ptr [rsp + 0x60], xmm6",
        "movaps xmmword ptr [rsp + 0x70], xmm7",
        "movaps xmmword ptr [rsp + 0x80], xmm8",
        "movaps xmmword ptr [rsp + 0x90], xmm9",
        "movaps xmmword ptr [rsp + 0xA0], xmm10",
        "movaps xmmword ptr [rsp + 0xB0], xmm11",
        "movaps xmmword ptr [rsp + 0xC0], xmm12",
        "movaps xmmword ptr [rsp + 0xD0], xmm13",
        "movaps xmmword ptr [rsp + 0xE0], xmm14",
        "movaps xmmword ptr [rsp + 0xF0], xmm15",

        // invalidate caches
        "wbinvd",

        // SYNCFULL
        "rdseed rax",

        // now do experiment execution
        "rdrand rax",

        // SYNCFULL
        "lfence",
        "sfence",
        "mfence",
        "rdseed r8",

        // save comparison values
        "pushfq",
        "push rax",
        "push rbx",
        "push rcx",
        "push rdx",
        "push rsi",
        "push rdi",
        "push r8",
        "push r9",
        "push r10",
        "push r11",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        "push rbp",
        "sub rsp, 0x100",
        "movaps xmmword ptr [rsp], xmm0",
        "movaps xmmword ptr [rsp + 0x10], xmm1",
        "movaps xmmword ptr [rsp + 0x20], xmm2",
        "movaps xmmword ptr [rsp + 0x30], xmm3",
        "movaps xmmword ptr [rsp + 0x40], xmm4",
        "movaps xmmword ptr [rsp + 0x50], xmm5",
        "movaps xmmword ptr [rsp + 0x60], xmm6",
        "movaps xmmword ptr [rsp + 0x70], xmm7",
        "movaps xmmword ptr [rsp + 0x80], xmm8",
        "movaps xmmword ptr [rsp + 0x90], xmm9",
        "movaps xmmword ptr [rsp + 0xA0], xmm10",
        "movaps xmmword ptr [rsp + 0xB0], xmm11",
        "movaps xmmword ptr [rsp + 0xC0], xmm12",
        "movaps xmmword ptr [rsp + 0xD0], xmm13",
        "movaps xmmword ptr [rsp + 0xE0], xmm14",
        "movaps xmmword ptr [rsp + 0xF0], xmm15",

        // SYNCFULL
        "rdseed rax",

        "add rsp, 0x180",
        "add rsp, 0x100",
        "pop rbp",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop r11",
        "pop r10",
        "pop r9",
        "pop r8",
        "pop rdi",
        "pop rsi",
        "pop rdx",
        "pop rcx",
        "pop rbx",
        "pop rax",
        "popfq",
        "sub rsp, 0x180",
        "sub rsp, 0x180",


        "pop rax",
        "mov [rcx + {registers_xmm0}], rax",
        "pop rax",
        "mov [rcx + {registers_xmm0b}], rax",
        "pop rax",
        "mov [rcx + {registers_xmm1}], rax",
        "pop rax",
        "mov [rcx + {registers_xmm1b}], rax",
        "pop rax",
        "mov [rcx + {registers_xmm2}], rax",
        "pop rax",
        "mov [rcx + {registers_xmm2b}], rax",
        "pop rax",
        "mov [rcx + {registers_xmm3}], rax",
        "pop rax",
        "mov [rcx + {registers_xmm3b}], rax",
        "pop rax",
        "mov [rcx + {registers_xmm4}], rax",
        "pop rax",
        "mov [rcx + {registers_xmm4b}], rax",
        "pop rax",
        "mov [rcx + {registers_xmm5}], rax",
        "pop rax",
        "mov [rcx + {registers_xmm5b}], rax",
        "pop rax",
        "mov [rcx + {registers_xmm6}], rax",
        "pop rax",
        "mov [rcx + {registers_xmm6b}], rax",
        "pop rax",
        "mov [rcx + {registers_xmm7}], rax",
        "pop rax",
        "mov [rcx + {registers_xmm7b}], rax",
        "pop rax",
        "mov [rcx + {registers_xmm8}], rax",
        "pop rax",
        "mov [rcx + {registers_xmm8b}], rax",
        "pop rax",
        "mov [rcx + {registers_xmm9}], rax",
        "pop rax",
        "mov [rcx + {registers_xmm9b}], rax",
        "pop rax",
        "mov [rcx + {registers_xmm10}], rax",
        "pop rax",
        "mov [rcx + {registers_xmm10b}], rax",
        "pop rax",
        "mov [rcx + {registers_xmm11}], rax",
        "pop rax",
        "mov [rcx + {registers_xmm11b}], rax",
        "pop rax",
        "mov [rcx + {registers_xmm12}], rax",
        "pop rax",
        "mov [rcx + {registers_xmm12b}], rax",
        "pop rax",
        "mov [rcx + {registers_xmm13}], rax",
        "pop rax",
        "mov [rcx + {registers_xmm13b}], rax",
        "pop rax",
        "mov [rcx + {registers_xmm14}], rax",
        "pop rax",
        "mov [rcx + {registers_xmm14b}], rax",
        "pop rax",
        "mov [rcx + {registers_xmm15}], rax",
        "pop rax",
        "mov [rcx + {registers_xmm15b}], rax",
        "pop rax",
        "mov [rcx + {registers_rbp}], rax",
        "pop rax",
        "mov [rcx + {registers_r15}], rax",
        "pop rax",
        "mov [rcx + {registers_r14}], rax",
        "pop rax",
        "mov [rcx + {registers_r13}], rax",
        "pop rax",
        "mov [rcx + {registers_r12}], rax",
        "pop rax",
        "mov [rcx + {registers_r11}], rax",
        "pop rax",
        "mov [rcx + {registers_r10}], rax",
        "pop rax",
        "mov [rcx + {registers_r9}], rax",
        "pop rax",
        "mov [rcx + {registers_r8}], rax",
        "pop rax",
        "mov [rcx + {registers_rdi}], rax",
        "pop rax",
        "mov [rcx + {registers_rsi}], rax",
        "pop rax",
        "mov [rcx + {registers_rdx}], rax",
        "pop rax",
        "mov [rcx + {registers_rcx}], rax",
        "pop rax",
        "mov [rcx + {registers_rbx}], rax",
        "pop rax",
        "mov [rcx + {registers_rax}], rax",
        "pop rax",
        "mov [rcx + {registers_rflags}], rax",

        "pop rax",
        "mov [rdx + {registers_xmm0}], rax",
        "pop rax",
        "mov [rdx + {registers_xmm0b}], rax",
        "pop rax",
        "mov [rdx + {registers_xmm1}], rax",
        "pop rax",
        "mov [rdx + {registers_xmm1b}], rax",
        "pop rax",
        "mov [rdx + {registers_xmm2}], rax",
        "pop rax",
        "mov [rdx + {registers_xmm2b}], rax",
        "pop rax",
        "mov [rdx + {registers_xmm3}], rax",
        "pop rax",
        "mov [rdx + {registers_xmm3b}], rax",
        "pop rax",
        "mov [rdx + {registers_xmm4}], rax",
        "pop rax",
        "mov [rdx + {registers_xmm4b}], rax",
        "pop rax",
        "mov [rdx + {registers_xmm5}], rax",
        "pop rax",
        "mov [rdx + {registers_xmm5b}], rax",
        "pop rax",
        "mov [rdx + {registers_xmm6}], rax",
        "pop rax",
        "mov [rdx + {registers_xmm6b}], rax",
        "pop rax",
        "mov [rdx + {registers_xmm7}], rax",
        "pop rax",
        "mov [rdx + {registers_xmm7b}], rax",
        "pop rax",
        "mov [rdx + {registers_xmm8}], rax",
        "pop rax",
        "mov [rdx + {registers_xmm8b}], rax",
        "pop rax",
        "mov [rdx + {registers_xmm9}], rax",
        "pop rax",
        "mov [rdx + {registers_xmm9b}], rax",
        "pop rax",
        "mov [rdx + {registers_xmm10}], rax",
        "pop rax",
        "mov [rdx + {registers_xmm10b}], rax",
        "pop rax",
        "mov [rdx + {registers_xmm11}], rax",
        "pop rax",
        "mov [rdx + {registers_xmm11b}], rax",
        "pop rax",
        "mov [rdx + {registers_xmm12}], rax",
        "pop rax",
        "mov [rdx + {registers_xmm12b}], rax",
        "pop rax",
        "mov [rdx + {registers_xmm13}], rax",
        "pop rax",
        "mov [rdx + {registers_xmm13b}], rax",
        "pop rax",
        "mov [rdx + {registers_xmm14}], rax",
        "pop rax",
        "mov [rdx + {registers_xmm14b}], rax",
        "pop rax",
        "mov [rdx + {registers_xmm15}], rax",
        "pop rax",
        "mov [rdx + {registers_xmm15b}], rax",
        "pop rax",
        "mov [rdx + {registers_rbp}], rax",
        "pop rax",
        "mov [rdx + {registers_r15}], rax",
        "pop rax",
        "mov [rdx + {registers_r14}], rax",
        "pop rax",
        "mov [rdx + {registers_r13}], rax",
        "pop rax",
        "mov [rdx + {registers_r12}], rax",
        "pop rax",
        "mov [rdx + {registers_r11}], rax",
        "pop rax",
        "mov [rdx + {registers_r10}], rax",
        "pop rax",
        "mov [rdx + {registers_r9}], rax",
        "pop rax",
        "mov [rdx + {registers_r8}], rax",
        "pop rax",
        "mov [rdx + {registers_rdi}], rax",
        "pop rax",
        "mov [rdx + {registers_rsi}], rax",
        "pop rax",
        "mov [rdx + {registers_rdx}], rax",
        "pop rax",
        "mov [rdx + {registers_rcx}], rax",
        "pop rax",
        "mov [rdx + {registers_rbx}], rax",
        "pop rax",
        "mov [rdx + {registers_rax}], rax",
        "pop rax",
        "mov [rdx + {registers_rflags}], rax",

        // go back to initial store
        "sub rsp, 0x180",

        // restore initial state
        "movaps xmm15, xmmword ptr [rsp + 0xF0]",
        "movaps xmm14, xmmword ptr [rsp + 0xE0]",
        "movaps xmm13, xmmword ptr [rsp + 0xD0]",
        "movaps xmm12, xmmword ptr [rsp + 0xC0]",
        "movaps xmm11, xmmword ptr [rsp + 0xB0]",
        "movaps xmm10, xmmword ptr [rsp + 0xA0]",
        "movaps xmm9, xmmword ptr [rsp + 0x90]",
        "movaps xmm8, xmmword ptr [rsp + 0x80]",
        "movaps xmm7, xmmword ptr [rsp + 0x70]",
        "movaps xmm6, xmmword ptr [rsp + 0x60]",
        "movaps xmm5, xmmword ptr [rsp + 0x50]",
        "movaps xmm4, xmmword ptr [rsp + 0x40]",
        "movaps xmm3, xmmword ptr [rsp + 0x30]",
        "movaps xmm2, xmmword ptr [rsp + 0x20]",
        "movaps xmm1, xmmword ptr [rsp + 0x10]",
        "movaps xmm0, xmmword ptr [rsp]",
        "add rsp, 0x100",
        "pop rbp",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop r11",
        "pop r10",
        "pop r9",
        "pop r8",
        "pop rdi",
        "pop rsi",
        "pop rdx",
        "pop rcx",
        "pop rbx",
        "pop rax",
        "popfq",

        "lfence",
        "mfence",

        out("rax") _,
        in("rcx") &mut after,
        in("rdx") &mut before,
        registers_rax = const mem::offset_of!(GuestRegisters, rax),
        registers_rcx = const mem::offset_of!(GuestRegisters, rcx),
        registers_rdx = const mem::offset_of!(GuestRegisters, rdx),
        registers_rbx = const mem::offset_of!(GuestRegisters, rbx),
        //registers_rsp = const mem::offset_of!(GuestRegisters, rsp),
        registers_rbp = const mem::offset_of!(GuestRegisters, rbp),
        registers_rsi = const mem::offset_of!(GuestRegisters, rsi),
        registers_rdi = const mem::offset_of!(GuestRegisters, rdi),
        registers_r8  = const mem::offset_of!(GuestRegisters, r8),
        registers_r9  = const mem::offset_of!(GuestRegisters, r9),
        registers_r10 = const mem::offset_of!(GuestRegisters, r10),
        registers_r11 = const mem::offset_of!(GuestRegisters, r11),
        registers_r12 = const mem::offset_of!(GuestRegisters, r12),
        registers_r13 = const mem::offset_of!(GuestRegisters, r13),
        registers_r14 = const mem::offset_of!(GuestRegisters, r14),
        registers_r15 = const mem::offset_of!(GuestRegisters, r15),
        //registers_rip = const mem::offset_of!(GuestRegisters, rip),
        registers_rflags = const mem::offset_of!(GuestRegisters, rflags),
        registers_xmm0 = const mem::offset_of!(GuestRegisters, xmm0),
        registers_xmm0b = const mem::offset_of!(GuestRegisters, xmm0)+0x08,
        registers_xmm1 = const mem::offset_of!(GuestRegisters, xmm1),
        registers_xmm1b = const mem::offset_of!(GuestRegisters, xmm1)+0x08,
        registers_xmm2 = const mem::offset_of!(GuestRegisters, xmm2),
        registers_xmm2b = const mem::offset_of!(GuestRegisters, xmm2)+0x08,
        registers_xmm3 = const mem::offset_of!(GuestRegisters, xmm3),
        registers_xmm3b = const mem::offset_of!(GuestRegisters, xmm3)+0x08,
        registers_xmm4 = const mem::offset_of!(GuestRegisters, xmm4),
        registers_xmm4b = const mem::offset_of!(GuestRegisters, xmm4)+0x08,
        registers_xmm5 = const mem::offset_of!(GuestRegisters, xmm5),
        registers_xmm5b = const mem::offset_of!(GuestRegisters, xmm5)+0x08,
        registers_xmm6 = const mem::offset_of!(GuestRegisters, xmm6),
        registers_xmm6b = const mem::offset_of!(GuestRegisters, xmm6)+0x08,
        registers_xmm7 = const mem::offset_of!(GuestRegisters, xmm7),
        registers_xmm7b = const mem::offset_of!(GuestRegisters, xmm7)+0x08,
        registers_xmm8 = const mem::offset_of!(GuestRegisters, xmm8),
        registers_xmm8b = const mem::offset_of!(GuestRegisters, xmm8)+0x08,
        registers_xmm9 = const mem::offset_of!(GuestRegisters, xmm9),
        registers_xmm9b = const mem::offset_of!(GuestRegisters, xmm9)+0x08,
        registers_xmm10 = const mem::offset_of!(GuestRegisters, xmm10),
        registers_xmm10b = const mem::offset_of!(GuestRegisters, xmm10)+0x08,
        registers_xmm11 = const mem::offset_of!(GuestRegisters, xmm11),
        registers_xmm11b = const mem::offset_of!(GuestRegisters, xmm11)+0x08,
        registers_xmm12 = const mem::offset_of!(GuestRegisters, xmm12),
        registers_xmm12b = const mem::offset_of!(GuestRegisters, xmm12)+0x08,
        registers_xmm13 = const mem::offset_of!(GuestRegisters, xmm13),
        registers_xmm13b = const mem::offset_of!(GuestRegisters, xmm13)+0x08,
        registers_xmm14 = const mem::offset_of!(GuestRegisters, xmm14),
        registers_xmm14b = const mem::offset_of!(GuestRegisters, xmm14)+0x08,
        registers_xmm15 = const mem::offset_of!(GuestRegisters, xmm15),
        registers_xmm15b = const mem::offset_of!(GuestRegisters, xmm15)+0x08,
        );
    }

    println!("Hooks enabled after: {}", hooks_enabled());

    before.rflags = 0x202;

    println!("RDX: {:x?}", before.rdx);

    let difference = before.difference(&after);
    if difference.is_empty() {
        println!("No difference detected");
    } else {
        for (affected_register, before, after) in difference.iter() {
            println!(
                "Register {:>6}:  {:x?} to {:x?}",
                affected_register, before, after
            );
        }
    }
}

#[inline(always)]
pub fn flush_decode_stream_buffer() {
    unsafe {
        asm!(
        // Align `ip` to a 64-byte boundary
        "lea {ip}, [rip]",
        "and {ip}, ~63",

        // Copy 512 bytes from `ip` to `ip`
        "mov {index}, 512",
        "mov {ptr}, {ip}",
        "2:",
        "mov {tmp:l}, byte ptr [{ptr}]",
        "mov byte ptr [{ptr}], {tmp:l}",
        "inc {ptr}",
        "dec {index}",
        "jnz 2b",

        // Execute sfence
        "sfence",

        // Flush cache lines
        "mov {index}, 0",
        "3:",
        "clflush byte ptr [{ip} + {index}]",
        "add {index}, 64",
        "cmp {index}, 512",
        "jne 3b",

        ip = out(reg) _,
        index = out(reg) _,
        tmp = out(reg) _,
            ptr = out(reg) _,
        );
    }
}
