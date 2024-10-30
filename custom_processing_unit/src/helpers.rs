use alloc::format;
use alloc::string::ToString;
use core::arch::asm;
use ucode_compiler::UcodePatchBlob;
use crate::{patches, Error};

#[inline(always)]
#[allow(unused)]
fn mfence() {
    unsafe { asm!("mfence", options(nostack)) }
}

#[inline(always)]
#[allow(unused)]
fn lfence() {
    unsafe { asm!("lfence", options(nostack)) }
}

#[inline(always)]
fn lmfence() {
    unsafe { asm!("lfence; mfence", options(nostack)) }
}

#[inline(always)]
#[allow(unused)]
fn wbinvd() {
    unsafe { asm!("wbinvd", options(nostack, preserves_flags)) }
}

#[inline(always)]
#[allow(unused)]
fn barrier() {
    unsafe {
        asm!(
        "mov {rbx_tmp}, rbx",
        "xor rax, rax",
        "xor rcx, rcx",
        "cpuid",
        "mov rbx, {rbx_tmp}",
        out("rax") _, rbx_tmp = out(reg) _, out("rcx") _, out("rdx") _,
        options(nostack, nomem)
        )
    }
}

fn udebug_read(command: usize, address: usize) -> usize {
    let mut res_high: usize;
    let mut res_low: usize;
    lmfence();
    unsafe {
        asm!(
        "mov {rbx_tmp}, rbx",
        ".byte 0x0f, 0x0e",
        "xchg {rbx_tmp}, rbx",
        rbx_tmp = out(reg) res_high,
        out("rdx") res_low,
        in("rcx") command,
        in("rax") address,
        options(nostack)
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
        "xchg {rbx_tmp}, rbx",
        ".byte 0x0f, 0x0f",
        "mov rbx, {rbx_tmp}",
        in("rcx") command,
        in("rax") address,
        rbx_tmp = in(reg) val_high,
        in("rdx") val_low,
        options(nostack)
        );
    }
    lmfence();
}

pub fn udebug_invoke(
    address: usize,
    res_a: &mut usize,
    res_b: &mut usize,
    res_c: &mut usize,
    res_d: &mut usize,
) {
    lmfence();
    unsafe {
        asm!(
        "xchg {rbx_tmp}, rbx",
        ".byte 0x0f, 0x0f",
        "xchg {rbx_tmp}, rbx",
        inout("rax") address => *res_a,
        rbx_tmp = inout(reg) 0usize => *res_b,
        inout("rcx") 0xd8usize => *res_c,
        inout("rdx") 0usize => *res_d,
        options(nostack)
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
        options(nostack, nomem, preserves_flags)
        );
    }
}

#[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CpuidResult {
    pub eax: u32,
    pub ebx: u32,
    pub ecx: u32,
    pub edx: u32,
}

impl CpuidResult {
    pub fn query(leaf: u32, sub_leaf: u32) -> CpuidResult {
        let eax;
        let ebx;
        let ecx;
        let edx;

        #[cfg(target_arch = "x86_64")]
        unsafe {
            asm!(
                "mov {0:r}, rbx",
                "cpuid",
                "xchg {0:r}, rbx",
                out(reg) ebx,
                inout("eax") leaf => eax,
                inout("ecx") sub_leaf => ecx,
                out("edx") edx,
                options(nostack, preserves_flags),
            );
        }
        CpuidResult { eax, ebx, ecx, edx }
    }
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

pub fn stgbuf_write(address: usize, value: usize) {
    udebug_write(0x80, address, value)
}

pub fn stgbuf_read(address: usize) -> usize {
    udebug_read(0x80, address)
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

#[allow(unused)]
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

#[allow(unused)]
pub fn ms_match_patch_write(addr: usize, val: usize) {
    ms_array_write(3, 0, 0, addr, val)
}

pub fn ms_const_write(addr: usize, val: usize) {
    ms_array_write(2, 0, 0, addr, val)
}

pub fn detect_glm_version() -> u32 {
    CpuidResult::query(0x1, 0).eax
}

pub fn patch_ucode(addr: usize, ucode_patch: &UcodePatchBlob) {
    // format: uop0, uop1, uop2, seqword
    // uop3 is fixed to a nop and cannot be overridden

    for i in 0..ucode_patch.len() {
        // patch ucode
        ms_patch_ram_write(
            crate::helpers::ucode_addr_to_patch_addr(addr + i * 4),
            ucode_patch[i][0],
        );
        ms_patch_ram_write(
            crate::helpers::ucode_addr_to_patch_addr(addr + i * 4) + 1,
            ucode_patch[i][1],
        );
        ms_patch_ram_write(
            crate::helpers::ucode_addr_to_patch_addr(addr + i * 4) + 2,
            ucode_patch[i][2],
        );

        // patch seqword
        ms_const_write(
            crate::helpers::ucode_addr_to_patch_seqword_addr(addr) + i,
            ucode_patch[i][3],
        );
    }
}

pub fn hook_match_and_patch(entry_idx: usize, ucode_addr: usize, patch_addr: usize) -> crate::Result<()> {
    if ucode_addr % 2 != 0 {
        return Err(Error::HookFailed("uop address must be even".to_string()).into());
    }
    if patch_addr % 2 != 0 || patch_addr < 0x7c00 {
        return Err(Error::HookFailed("patch uop address must be even and >0x7c00".to_string()).into());
    }

    // todo more advanced range checks

    //TODO: try to hook odd addresses!!
    let poff = (patch_addr - 0x7c00) / 2;
    let patch_value = 0x3e000000 | (poff << 16) | ucode_addr | 1;

    let match_patch_hook = patches::match_patch_hook;
    patch_ucode(match_patch_hook.addr, match_patch_hook.ucode_patch);

    let mut res_a = 0;
    let mut res_b = 0;
    let mut res_c = 0;
    let mut res_d = 0;
    stgbuf_write(0xb800, patch_value); // write value to tmp0
    stgbuf_write(0xb840, entry_idx*2); // write idx to tmp1

    udebug_invoke(match_patch_hook.addr, &mut res_a, &mut res_b, &mut res_c, &mut res_d);

    stgbuf_write(0xb800, 0); // restore tmp0
    stgbuf_write(0xb840, 0); // restore tmp1

    if res_a != 0x0000133700001337 {
        return Err(Error::HookFailed(format!("invoke({:08x}) = {:016x}, {:016x}, {:016x}, {:016x}",
                                                 match_patch_hook.addr, res_a, res_b, res_c, res_d)).into());
    }

    Ok(())
}
