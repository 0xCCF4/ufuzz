//! # Speculation X86
//!
//! An experiment for analyzing and testing x86 architecture-level speculation behavior.

#![no_main]
#![no_std]
#![allow(named_asm_labels)]
#![allow(unsafe_attr_outside_unsafe)]
extern crate alloc;

use alloc::vec::Vec;
use core::arch::asm;
use custom_processing_unit::{
    CustomProcessingUnit, HookGuard, StagingBufferAddress, apply_hook_patch_func, apply_patch, hook,
};
use data_types::addresses::MSRAMHookIndex;
use log::info;
use speculation_x86::patches;
use uefi::{Status, entry, print, println};
use x86_perf_counter::{
    INSTRUCTIONS_RETIRED, MS_DECODED_MS_ENTRY, PerformanceCounter, UOPS_ISSUED_ANY,
    UOPS_RETIRED_ANY,
};

#[entry]
unsafe fn main() -> Status {
    uefi::helpers::init().unwrap();
    print!("Hello world! ");

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

    let disable_hooks = HookGuard::disable_all(); // will be dropped on end of method
    let _stg_addr = StagingBufferAddress::Raw(0xba00);

    if let Err(err) = apply_patch(&patches::experiment::PATCH) {
        println!("Failed to apply patch: {:?}", err);
        return Status::ABORTED;
    }

    if let Err(err) = hook(
        apply_hook_patch_func(),
        MSRAMHookIndex::ZERO,
        0x428,
        patches::experiment::LABEL_ENTRY,
        true,
    ) {
        println!("Failed to apply hook: {:?}", err);
        return Status::ABORTED;
    }

    {
        let enable_hooks = HookGuard::enable_all();
        //test_speculation("Instruction", spec_window);

        unsafe { spec_ucode() };

        enable_hooks.restore();
    }

    disable_hooks.restore();

    Status::SUCCESS
}

#[inline(never)]
pub unsafe fn test_speculation(title: &str, func: unsafe fn(bool, bool) -> u64) {
    let n = 2000; // Replace this with dynamic calculation if needed

    let mut results_true = Vec::with_capacity(n);
    let mut sum_true = 0;

    for i in 0..n {
        let result = unsafe { func(true, i == 0) };
        sum_true += result;
        results_true.push(result);
    }

    print!(
        "{title} Average attack:    {:<6.2} ",
        sum_true as f64 / n as f64
    );
    for i in 0..5.min(n) {
        print!("{:<3} ", results_true[i]);
    }
    print!(".. ");
    for i in n.saturating_sub(5)..n {
        print!("{:<3} ", results_true[i]);
    }
    println!();

    let mut results_false = Vec::with_capacity(n);
    let mut sum_false = 0;

    for i in 0..n {
        let result = unsafe { func(false, i == 0) };
        sum_false += result;
        results_false.push(result);
    }

    print!(
        "{title} Average base-line: {:<6.2} ",
        sum_false as f64 / n as f64
    );
    for i in 0..5.min(n) {
        print!("{:<3} ", results_false[i]);
    }
    print!(".. ");
    for i in n.saturating_sub(5)..n {
        print!("{:<3} ", results_false[i]);
    }
    println!();
}

pub unsafe fn spec_ucode() {
    let result: u64;

    let mut pmc_ms_entry = unsafe { PerformanceCounter::new(0) };
    pmc_ms_entry
        .event()
        .apply_perf_event_specifier(MS_DECODED_MS_ENTRY);
    pmc_ms_entry.event().set_user_mode(true);
    pmc_ms_entry.event().set_os_mode(true);
    pmc_ms_entry.reset();

    let mut pmc_uops_issued = unsafe { PerformanceCounter::new(1) };
    pmc_uops_issued
        .event()
        .apply_perf_event_specifier(UOPS_ISSUED_ANY);
    pmc_uops_issued.event().set_user_mode(true);
    pmc_uops_issued.event().set_os_mode(true);
    pmc_uops_issued.reset();

    let mut pmc_instructions_retired = unsafe { PerformanceCounter::new(2) };
    pmc_instructions_retired
        .event()
        .apply_perf_event_specifier(INSTRUCTIONS_RETIRED);
    pmc_instructions_retired.event().set_user_mode(true);
    pmc_instructions_retired.event().set_os_mode(true);
    pmc_instructions_retired.reset();

    let mut pmc_uops_retired = unsafe { PerformanceCounter::new(3) };
    pmc_uops_retired
        .event()
        .apply_perf_event_specifier(UOPS_RETIRED_ANY);
    pmc_uops_retired.event().set_user_mode(true);
    pmc_uops_retired.event().set_os_mode(true);
    pmc_uops_retired.reset();

    pmc_instructions_retired.enable();
    pmc_uops_retired.enable();
    pmc_uops_issued.enable();
    pmc_ms_entry.enable();

    unsafe { asm!("rdrand rax", out("rax") result) };

    pmc_ms_entry.disable();
    pmc_uops_issued.disable();
    pmc_uops_retired.disable();
    pmc_instructions_retired.disable();

    println!(
        "MSROM: {} uISSUE: {} uRETIRE: {} uSPEC: {} iRETIRE: {} Result {:04x}",
        pmc_ms_entry.read(),
        pmc_uops_issued.read(),
        pmc_uops_retired.read(),
        pmc_uops_retired.read() - pmc_uops_issued.read(),
        pmc_instructions_retired.read(),
        result
    );
}

#[inline(never)]
pub unsafe fn spec_window(attack: bool, pmc: bool) -> u64 {
    let difference: u64;

    let mut q = [0u32; 256];
    q[0] = 0x22;

    let mut pmc_ms_entry = unsafe { PerformanceCounter::new(0) };
    pmc_ms_entry
        .event()
        .apply_perf_event_specifier(MS_DECODED_MS_ENTRY);
    pmc_ms_entry.event().set_user_mode(true);
    pmc_ms_entry.event().set_os_mode(true);
    pmc_ms_entry.reset();

    let mut pmc_uops_issued = unsafe { PerformanceCounter::new(1) };
    pmc_uops_issued
        .event()
        .apply_perf_event_specifier(UOPS_ISSUED_ANY);
    pmc_uops_issued.event().set_user_mode(true);
    pmc_uops_issued.event().set_os_mode(true);
    pmc_uops_issued.reset();

    let mut pmc_instructions_retired = unsafe { PerformanceCounter::new(2) };
    pmc_instructions_retired
        .event()
        .apply_perf_event_specifier(INSTRUCTIONS_RETIRED);
    pmc_instructions_retired.event().set_user_mode(true);
    pmc_instructions_retired.event().set_os_mode(true);
    pmc_instructions_retired.reset();

    let mut pmc_uops_retired = unsafe { PerformanceCounter::new(3) };
    pmc_uops_retired
        .event()
        .apply_perf_event_specifier(UOPS_RETIRED_ANY);
    pmc_uops_retired.event().set_user_mode(true);
    pmc_uops_retired.event().set_os_mode(true);
    pmc_uops_retired.reset();

    unsafe {
        asm!("wbinvd");

        for i in (0..q.len()).step_by(64) {
            asm!("clflush [{}]", in(reg) &q[i]);
        }

        asm!("sfence", "lfence", "sfence", "lfence");
    }

    // https://blog.can.ac/2021/03/22/speculating-x86-64-isa-with-one-weird-trick/
    flush_decode_stream_buffer();

    pmc_instructions_retired.enable();
    pmc_uops_retired.enable();
    pmc_uops_issued.enable();
    pmc_ms_entry.enable();

    unsafe {
        asm!(
        // Speculation template based on https://blog.can.ac/2021/03/22/speculating-x86-64-isa-with-one-weird-trick/

        "xor rdx,rdx",
        "cmp {attack}, 0",
        "jz 2f",
        "call 4f",

        // Speculative window

        "lea rdx, [{q_ptr}]",
        "mov rdx, [rdx]",
        "ud2",

        // Speculative window end

        "4:",
        "lea rax, [rip+2f]",
        "xchg [rsp], rax",
        "ret",
        "2:",
        "xor rax, rax",
        "mfence",
        "lfence",
        "rdtsc",
        "lfence",
        "shl rdx, 32",
        "or rax, rdx",
        "mov {before}, rax",
        "xor rax, rax",
        "lea rdx, [{q_ptr}]",
        "mov rdx, [rdx]",
        "mfence",
        "lfence",
        "rdtsc",
        "lfence",
        "shl rdx, 32",
        "or rax, rdx",
        "mov {after}, rax",
        "xor rax, rax",
        "sub {after}, {before}",
        "mov {difference}, {after}",
        attack = in(reg) if attack { 1u64 } else { 0u64 },
        q_ptr = in(reg) (&q as *const u32 as usize),
        out("rdx") _,
        out("rax") _,
        out("rcx") _,
        before = out(reg) _,
        after = out(reg) _,
        difference = out(reg) difference,
        );
    }

    pmc_ms_entry.disable();
    pmc_uops_issued.disable();
    pmc_uops_retired.disable();
    pmc_instructions_retired.disable();

    if pmc {
        println!(
            "PMC {}: MSROM: {} uISSUE: {} uRETIRE: {} iRETIRE: {}",
            if attack { "Attack" } else { "Baseline" },
            pmc_ms_entry.read(),
            pmc_uops_issued.read(),
            pmc_uops_retired.read(),
            pmc_instructions_retired.read()
        );
    }

    difference
}

#[inline(always)]
pub fn flush_decode_stream_buffer() {
    unsafe {
        asm!(
        // Speculation template based on https://blog.can.ac/2021/03/22/speculating-x86-64-isa-with-one-weird-trick/

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
