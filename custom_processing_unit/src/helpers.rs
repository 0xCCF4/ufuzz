use crate::arch;
use std::arch::asm;
use std::arch::x86_64::CpuidResult;

#[inline(always)]
fn mfence() {
    unsafe { asm!("mfence", options(nostack)) }
}

#[inline(always)]
fn lfence() {
    unsafe { asm!("lfence", options(nostack)) }
}

#[inline(always)]
fn lmfence() {
    unsafe { asm!("lfence; mfence", options(nostack)) }
}

#[inline(always)]
fn wbinvd() {
    unsafe { asm!("wbinvd", options(nostack)) }
}

#[inline(always)]
fn barrier() {
    unsafe {
        asm!(
        "push rbx",
        "xor rax, rax",
        "xor rcx, rcx",
        "cpuid",
        "pop rbx",
        out("rax") _, out("rcx") _, out("rdx") _,
        )
    }
}

fn udebug_read(command: usize, address: usize) -> usize {
    let mut res_high: usize;
    let mut res_low: usize;
    lmfence();
    unsafe {
        asm!(
        "push rbx",
        ".byte 0x0f, 0x0e",
        "mov {rbx_tmp}, rbx",
        "pop rbx",
        rbx_tmp = out(reg) res_high,
        out("rdx") res_low,
        in("rcx") command,
        in("rax") address,
        );
    }
    lmfence();
    (res_high << 32) | res_low
}

fn udebug_write(command: usize, address: usize, value: usize) {
    let val_high = value >> 32;
    let val_low = value as u32;
    lmfence();
    unsafe {
        asm!(
        "push rbx",
        "mov rbx, {rbx_tmp}",
        ".byte 0x0f, 0x0f",
        "pop rbx",
        in("rcx") command,
        in("rax") address,
        rbx_tmp = in(reg) val_high,
        in("rdx") val_low,
        );
    }
    lmfence();
}

pub fn udebug_invoke(
    address: usize,
    resA: &mut usize,
    resB: &mut usize,
    resC: &mut usize,
    resD: &mut usize,
) {
    lmfence();
    unsafe {
        asm!(
        "push rbx",
        "xor rbx, rbx",
        ".byte 0x0f, 0x0f",
        "mov {rbx_tmp}, rbx",
        "pop rbx",
        lateout("rax") *resA,
        rbx_tmp = lateout(reg) *resB,
        lateout("rcx") *resC,
        lateout("rdx") *resD,
        in("rax") address,
        in("rcx") 0xd8,
        in("rdx") 0,
        );
    }
    lmfence();
}

#[inline(always)]
fn wrmsr(msr: u32, value: u64) {
    let low = (value & 0xFFFFFFFF) as u32;
    let high = (value >> 32) as u32;
    unsafe {
        asm!(
        "wrmsr",
        in("ecx") msr,
        in("eax") low,
        in("edx") high,
        options(nostack, nomem)
        );
    }
}

fn cpuid(leaf: u32, subleaf: u32) -> CpuidResult {
    let arch::CpuidResult { eax, ebx, ecx, edx } = unsafe { arch::__cpuid_count(leaf, subleaf) };
    return CpuidResult { eax, ebx, ecx, edx };
}

pub fn activate_udebug_insts() {
    wrmsr(0x1e6, 0x200);
}

pub fn crbus_read(address: usize) -> usize {
    udebug_read(0, address)
}

pub fn crbus_write(address: usize, value: usize) -> usize {
    udebug_write(0, address, value);
    udebug_read(0, address)
}

fn ldat_array_write(
    pdat_reg: usize,
    array_sel: usize,
    bank_sel: usize,
    dword_idx: usize,
    fast_addr: usize,
    val: usize,
) {
    // maybe signal that we are patching (seen in U2270)
    let prev = crbus_read(0x692);
    crbus_write(0x692, prev | 1);

    crbus_write(
        pdat_reg + 1,
        0x30000 | ((dword_idx & 0xf) << 12) | ((array_sel & 0xf) << 8) | (bank_sel & 0xf),
    );
    crbus_write(pdat_reg, 0x000000 | (fast_addr & 0xffff));
    crbus_write(pdat_reg + 4, val & 0xffffffff);
    crbus_write(pdat_reg + 5, (val >> 32) & 0xffff);
    crbus_write(pdat_reg + 1, 0);

    crbus_write(0x692, prev);
}

pub fn ucode_addr_to_patch_addr(addr: usize) -> usize {
    let base = addr - 0x7c00;
    // the last *4 does not make any sense but the CPU divides the address where
    // to write by 4, still unknown reasons
    ((base % 4) * 0x80 + (base / 4)) * 4
}

fn patch_addr_to_ucode_addr(addr: usize) -> usize {
    // NOTICE: the ucode_addr_to_patch_addr has a *4 more, so this will not be
    // the inverse
    let mul = addr % 0x80;
    let off = addr / 0x80;
    0x7c00 + mul * 4 + off
}

pub fn ucode_addr_to_patch_seqword_addr(addr: usize) -> usize {
    let base = addr - 0x7c00;
    let seq_addr = (base % 4) * 0x80 + (base / 4);
    seq_addr % 0x80
}

fn ms_array_write(
    array_sel: usize,
    bank_sel: usize,
    dword_idx: usize,
    fast_addr: usize,
    val: usize,
) {
    ldat_array_write(0x6a0, array_sel, bank_sel, dword_idx, fast_addr, val)
}

pub fn ms_patch_ram_write(addr: usize, val: usize) {
    ms_array_write(4, 0, 0, addr, val)
}

pub fn ms_match_patch_write(addr: usize, val: usize) {
    ms_array_write(3, 0, 0, addr, val)
}

pub fn ms_const_write(addr: usize, val: usize) {
    ms_array_write(2, 0, 0, addr, val)
}

pub fn detect_glm_version() -> u32 {
    cpuid(0x1, 0).eax
}
