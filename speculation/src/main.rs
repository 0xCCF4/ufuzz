#![no_main]
#![no_std]
#![allow(named_asm_labels)]

use core::arch;
use core::arch::asm;
use custom_processing_unit::{
    CustomProcessingUnit, HookGuard, StagingBufferAddress, apply_hook_patch_func, apply_patch,
    hook, stgbuf_read, stgbuf_write,
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

    let mut sum_true = 0;
    for i in 0..200 {
        let result = spec(true);
        sum_true += result;
    }
    println!("\nAverage (true): {:0.2}", sum_true as f64 / 200.0);

    let mut sum_false = 0;
    for i in 0..200 {
        let result = spec(false);
        sum_false += result;
    }
    println!("\nAverage (false): {:0.2}", sum_false as f64 / 200.0);

    disable_hooks.restore();

    Status::SUCCESS
}

#[inline(never)]
pub unsafe fn spec(prepare: bool) -> u64 {
    let mut a = [0u32; 128];
    a[127] = 1;
    let a = a;

    let mut p = [0u32; 1024 * 2];
    p[1024] = 1;
    let p = p;

    let mut q = [0u32; 256];
    q[0] = 0x22;

    asm!("wbinvd");

    for i in (0..p.len()).step_by(64) {
        asm!("clflush [{}]", in(reg) &p[i]);
    }
    for i in (0..a.len()).step_by(64) {
        asm!("clflush [{}]", in(reg) &a[i]);
    }
    for i in (0..q.len()).step_by(64) {
        asm!("clflush [{}]", in(reg) &q[i]);
    }

    asm!("sfence", "lfence", "sfence", "lfence",);

    if prepare {
        for i in 0..129 {
            asm!("mov rax, rax");
            asm!("mov rax, rax");
            asm!("mov rax, rax");
            if p[(i >> 7) << 10] < 1 {
                asm!("mov rax, rax");
                asm!("mov rax, rax");
                asm!("mov rax, rax");
                if i == 128 {
                    asm!(
                    //"nop",
                    "mov rdx, [{q_ptr}]",
                    q_ptr = in(reg) (&q as *const u32 as usize) & (0usize.wrapping_sub(i >> 7)),
                    out("rdx") _,
                    );
                }

                /*if i == 128 {
                    asm!(
                    //"nop",
                    "mov rdx, [{q_ptr}]",
                    q_ptr = in(reg) &q,
                    out("rdx") _,
                    );
                }*/
                asm!("xchg rax, [rsp]");
                asm!("xchg rax, [rsp]");
            }
        }
    }

    asm!("sfence", "lfence");
    let time_before = arch::x86_64::_rdtsc();
    asm!("sfence", "lfence");

    asm!(
        "lea rdx, [{q_ptr}]",
        "mov rax, [rdx]",
        q_ptr = in(reg) &q,
        out("rdx") _,
    );

    asm!("sfence", "lfence");
    let time_after = arch::x86_64::_rdtsc();
    asm!("sfence", "lfence");

    time_after.saturating_sub(time_before)
}

pub fn speculation1() {
    let hooks = HookGuard::enable_all();

    unsafe {
        asm!(
        "xor rax, rax",
        "2:",
        "inc rax",
        "cmp rax, 0x40",
        "je 3f",
        "rdrand rax",
        "jmp 2b",
        "3:",
        "nop",
        out("rax") _,
        out("rdx") _,
        );
    }

    hooks.restore();
}

pub fn speculation2() {
    unsafe {
        asm!(
            // Align `ip` to a 64-byte boundary
            "lea {ip}, [rip]",
            "and {ip}, ~63",

            // Copy 512 bytes from `ip` to `ip`
            "mov rcx, 512",
            "mov {i}, {ip}",
            "2:",
            "mov al, byte ptr [{i}]",
            "mov byte ptr [{i}], al",
            "inc {i}",
            "loop 2b",

            // Execute sfence
            "sfence",

            // Flush cache lines
            "mov rcx, 0",
            "3:",
            "clflush byte ptr [{ip} + rcx]",
            "add rcx, 64",
            "cmp rcx, 512",
            "jne 3b",

            "call 4f",
            // Speculative window
            "rdrand rdx",
            "ud2",
            "4:",
            "lea rax, [rip+5f]",
            "xchg [rsp], rax",
            "ret",
            "5:",

            ip = out(reg) _,
            i = out(reg) _,
            out("rcx") _,
            out("al") _,
            out("rdx") _,
        );
    }
}
