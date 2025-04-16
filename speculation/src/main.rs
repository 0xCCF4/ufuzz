#![no_main]
#![no_std]
#![allow(named_asm_labels)]
#![allow(unsafe_attr_outside_unsafe)]
extern crate alloc;

use alloc::vec::Vec;
use core::arch;
use core::arch::asm;
use custom_processing_unit::{
    CustomProcessingUnit, HookGuard, StagingBufferAddress, apply_hook_patch_func, apply_patch,
    hook, lmfence, stgbuf_read, stgbuf_write,
};
use data_types::addresses::MSRAMHookIndex;
use log::info;
use speculation::patches;
use uefi::{Status, entry, print, println};

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

    let disable_hooks = HookGuard::disable_all(); // will be dropped on end of method
    let stg_addr = StagingBufferAddress::Raw(0xba00);

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
        test_speculation("Instruction", spec_window);

        let mut val = 0;
        asm!("rdrand rdx", out("rdx") val);
        println!("{:04x}", val);

        enable_hooks.restore();
    }

    disable_hooks.restore();

    Status::SUCCESS
}

#[inline(never)]
pub unsafe fn test_speculation(title: &str, func: unsafe fn(bool) -> u64) {
    let n = 2000; // Replace this with dynamic calculation if needed

    let mut results_true = Vec::with_capacity(n);
    let mut sum_true = 0;

    for i in 0..n {
        let result = func(true);
        sum_true += result;
        results_true.push(result);
    }

    print!(
        "{title} Average attack:    {:<6.2} ",
        sum_true as f64 / n as f64
    );
    for i in 0..5 {
        print!("{:<3} ", results_true[i]);
    }
    print!(".. ");
    for i in (n - 5)..n {
        print!("{:<3} ", results_true[i]);
    }
    println!();

    let mut results_false = Vec::with_capacity(n);
    let mut sum_false = 0;

    for i in 0..n {
        let result = func(false);
        sum_false += result;
        results_false.push(result);
    }

    print!(
        "{title} Average base-line: {:<6.2} ",
        sum_false as f64 / n as f64
    );
    for i in 0..5 {
        print!("{:<3} ", results_false[i]);
    }
    print!(".. ");
    for i in (n - 5)..n {
        print!("{:<3} ", results_false[i]);
    }
    println!();
}

#[inline(never)]
pub unsafe fn spec_window(attack: bool) -> u64 {
    let difference: u64;

    let mut q = [0u32; 256];
    q[0] = 0x22;

    asm!("wbinvd");

    for i in (0..q.len()).step_by(64) {
        asm!("clflush [{}]", in(reg) &q[i]);
    }

    asm!("sfence", "lfence", "sfence", "lfence");

    // https://blog.can.ac/2021/03/22/speculating-x86-64-isa-with-one-weird-trick/
    flush_decode_stream_buffer();

    asm!(
        "xor rdx,rdx",
        "cmp {attack}, 0",
        "jz 2f",

        // payload here

        "call 4f",

        // Speculative window

        "lea rdx, [{q_ptr}]",
        "mov rdx, [rdx]",

        // Speculative window end

        "ud2",

        "4:",
        "lea rax, [rip+2f]",
        "xchg [rsp], rax",
        "ret",

        // payload end

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

        attack = in(reg) if attack {1} else {0},
        q_ptr = in(reg) (&q as *const u32 as usize),
        out("rdx") _,
        out("rax") _,
        out("rcx") _,
        before = out(reg) _,
        after = out(reg) _,
        difference = out(reg) difference,
    );

    difference
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
