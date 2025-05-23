#![feature(new_zeroed_alloc)]
#![allow(incomplete_features)]
#![feature(generic_const_exprs)]
#![feature(stmt_expr_attributes)]
#![feature(proc_macro_hygiene)]
#![no_std]

pub mod controller_connection;
pub mod patches;

extern crate alloc;

use crate::controller_connection::ControllerConnection;
use alloc::vec::Vec;
use alloc::{format, vec};
use core::arch::asm;
use core::mem;
use custom_processing_unit::{
    apply_ldat_read_func, ms_patch_instruction_read, ms_patch_instruction_write, patch_ucode,
    HookGuard,
};
use data_types::addresses::Address;
use fuzzer_data::SpeculationResult;
use hypervisor::state::GuestRegisters;
use itertools::Itertools;
use log::{trace, Level};
use ucode_compiler_dynamic::instruction::Instruction;
use ucode_compiler_dynamic::sequence_word::SequenceWord;
use uefi::println;
use x86::msr::{IA32_PERFEVTSEL0, IA32_PERFEVTSEL1, IA32_PERFEVTSEL2, IA32_PERFEVTSEL3};
use x86_perf_counter::{PerfEventSpecifier, PerformanceCounter};

pub fn check_if_pmc_stable(
    udp: &mut ControllerConnection,
    perf_counter_setup: Vec<PerfEventSpecifier>,
) -> Vec<bool> {
    let _ = udp.log_reliable(Level::Trace, "check pmc stable");

    let mut results = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];

    for _ in 0..10 {
        let result = execute_speculation(
            udp,
            [Instruction::NOP, Instruction::NOP, Instruction::NOP],
            SequenceWord::NOP,
            perf_counter_setup.clone(),
        )
        .perf_counters;

        results[0].push(result[0]);
        results[1].push(result[1]);
        results[2].push(result[2]);
        results[3].push(result[3]);
    }

    results
        .into_iter()
        .map(|x| x.iter().all_equal())
        .collect_vec()
}

pub fn execute_speculation(
    udp: &mut ControllerConnection,
    triad: [Instruction; 3],
    sequence_word: SequenceWord,
    perf_counter_setup: Vec<PerfEventSpecifier>,
) -> SpeculationResult {
    let _ = udp.log_reliable(
        Level::Info,
        format!(
            "execute speculation_x86: {} {:04x}",
            triad[0].opcode(),
            triad[0].assemble()
        ),
    );
    trace!(
        "Execute speculation_x86: {} {:04x}",
        triad[0].opcode(),
        triad[0].assemble()
    );

    let sequence_word = match sequence_word.assemble() {
        Ok(word) => word,
        Err(e) => {
            let _ = udp.log_reliable(
                Level::Error,
                &format!("Failed to assemble sequence word: {:?}", e),
            );
            return SpeculationResult {
                arch_before: GuestRegisters::default(),
                arch_after: GuestRegisters::default(),
                perf_counters: Vec::new(),
            };
        }
    };

    ms_patch_instruction_write(
        patches::patch::LABEL_SPECULATIVE_WINDOW,
        triad[0].assemble() as usize,
    );

    unsafe {
        asm!("rdseed rax", out("rax")_); // SYNCFULL
    }

    let perf_counter_setup: [Option<PerfEventSpecifier>; 4] = {
        let mut setup = perf_counter_setup.into_iter().map(Some).collect::<Vec<_>>();
        setup.truncate(4);
        while setup.len() < 4 {
            setup.push(None);
        }
        [
            setup[0].clone(),
            setup[1].clone(),
            setup[2].clone(),
            setup[3].clone(),
        ]
    };

    let result = collect_perf_counters_values(perf_counter_setup);
    result
}

#[inline(always)]
fn collect_perf_counters_values(
    perf_counter_setup: [Option<PerfEventSpecifier>; 4],
) -> SpeculationResult {
    let mut initial_state = GuestRegisters::default();
    let mut final_state = GuestRegisters::default();

    let mut perf0 = if let Some(event) = &perf_counter_setup[0] {
        PerformanceCounter::from_perf_event_specifier(0, event)
    } else {
        PerformanceCounter::new(0)
    };
    let mut perf1 = if let Some(event) = &perf_counter_setup[1] {
        PerformanceCounter::from_perf_event_specifier(1, event)
    } else {
        PerformanceCounter::new(1)
    };
    let mut perf2 = if let Some(event) = &perf_counter_setup[2] {
        PerformanceCounter::from_perf_event_specifier(2, event)
    } else {
        PerformanceCounter::new(2)
    };
    let mut perf3 = if let Some(event) = &perf_counter_setup[3] {
        PerformanceCounter::from_perf_event_specifier(3, event)
    } else {
        PerformanceCounter::new(3)
    };

    perf0
        .event()
        .set_enable_counters(perf_counter_setup[0].is_some());
    perf1
        .event()
        .set_enable_counters(perf_counter_setup[1].is_some());
    perf2
        .event()
        .set_enable_counters(perf_counter_setup[2].is_some());
    perf3
        .event()
        .set_enable_counters(perf_counter_setup[3].is_some());

    let guard = HookGuard::enable_all();

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

        // enable perf counters

        "push rax",
        "push rcx",
        "push rdx",
        "pushfq",

        "xor edx, edx",

        "mov ecx, {msr_sel_offset0}",
        "mov rdx, r8",
        "mov eax, edx",
        "shr rdx, 32",
        "wrmsr",

        "mov ecx, {msr_sel_offset1}",
        "mov rdx, r9",
        "mov eax, edx",
        "shr rdx, 32",
        "wrmsr",

        "mov ecx, {msr_sel_offset2}",
        "mov rdx, r10",
        "mov eax, edx",
        "shr rdx, 32",
        "wrmsr",

        "mov ecx, {msr_sel_offset3}",
        "mov rdx, r11",
        "mov eax, edx",
        "shr rdx, 32",
        "wrmsr",

        "popfq",
        "pop rdx",
        "pop rcx",
        "pop rax",

        // invalidate caches
        "wbinvd",

        // SYNCFULL
        "rdseed rax",

        // now do experiment execution
        "rdrand rax",

        // SYNCFULL
        "rdseed rax",

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

        // disable perf counters
        "xor edx, edx",

        "xor edx, edx",
        "mov ecx, {msr_sel_offset0}",
        "mov eax, 0x0000",
        "wrmsr",

        "mov ecx, {msr_sel_offset1}",
        "mov eax, 0x0000",
        "wrmsr",

        "mov ecx, {msr_sel_offset2}",
        "mov eax, 0x0000",
        "wrmsr",

        "mov ecx, {msr_sel_offset3}",
        "mov eax, 0x0000",
        "wrmsr",

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
        in("rcx") &mut final_state,
        in("rdx") &mut initial_state,
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
        in("r8") perf0.event().0,
        in("r9") perf1.event().0,
        in("r10") perf2.event().0,
        in("r11") perf3.event().0,
        msr_sel_offset0 = const IA32_PERFEVTSEL0,
        msr_sel_offset1 = const IA32_PERFEVTSEL1,
        msr_sel_offset2 = const IA32_PERFEVTSEL2,
        msr_sel_offset3 = const IA32_PERFEVTSEL3,
        );
    }

    guard.restore();

    SpeculationResult {
        perf_counters: vec![perf0.read(), perf1.read(), perf2.read(), perf3.read()],
        arch_after: final_state,
        arch_before: initial_state,
    }
}
