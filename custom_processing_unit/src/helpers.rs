use crate::Error;
use crate::StagingBufferAddress::{RegTmp0, RegTmp1, RegTmp2};
#[cfg(feature = "nostd")]
use alloc::format;
#[cfg(feature = "nostd")]
use alloc::string::ToString;
use core::arch::asm;
use core::fmt;
use core::fmt::{Display, Formatter};
use data_types::addresses::{
    Address, MSRAMAddress, MSRAMHookIndex, MSRAMInstructionPartReadAddress,
    MSRAMInstructionPartWriteAddress, MSRAMSequenceWordAddress, UCInstructionAddress,
};
use data_types::patch::{Patch, UcodePatchBlob};
use log::trace;
#[cfg(not(feature = "nostd"))]
use std::format;
#[cfg(not(feature = "nostd"))]
use std::string::ToString;

#[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum StagingBufferAddress {
    RegTmp0 = 0xb800,
    RegTmp1 = 0xb840,
    RegTmp2 = 0xb880,
    // todo: check further values
}

impl Address for StagingBufferAddress {
    fn address(&self) -> usize {
        *self as usize
    }
}

impl Display for StagingBufferAddress {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[inline(always)]
#[allow(unused)]
pub fn mfence() {
    unsafe { asm!("mfence", options(nostack)) }
}

#[inline(always)]
#[allow(unused)]
pub fn lfence() {
    unsafe { asm!("lfence", options(nostack)) }
}

#[inline(always)]
pub fn lmfence() {
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

// #[inline(always)]
fn udebug_read(command: usize, address: usize) -> usize {
    if cfg!(feature = "emulation") {
        let res_high: usize = 0;
        let res_low: usize;
        unsafe {
            asm!(
            "nop",
            out("rdx") res_low,
            in("rcx") command,
            in("rax") address,
            options(nostack)
            );
        }
        return res_low | (res_high << 32);
    }

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

// #[inline(always)]
fn udebug_write(command: usize, address: usize, value: usize) {
    if cfg!(feature = "emulation") {
        let val_low = value as u32;
        lmfence();
        unsafe {
            asm!(
            "nop",
            in("rcx") command,
            in("rax") address,
            in("rdx") val_low,
            options(nostack)
            );
        }
        lmfence();
        return;
    }

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

// #[inline(always)]
pub fn udebug_invoke(
    address: UCInstructionAddress,
    res_a: &mut usize,
    res_b: &mut usize,
    res_c: &mut usize,
    res_d: &mut usize,
) {
    if cfg!(feature = "emulation") {
        lmfence();
        unsafe {
            asm!(
            "nop",
            inout("rax") address.address() => *res_a,
            inout("rcx") 0xd8usize => *res_c,
            inout("rdx") 0usize => *res_d,
            options(nostack)
            );
        }
        lmfence();
        *res_a = 0;
        *res_b = 0;
        *res_c = 0;
        *res_d = 0;
        return;
    }

    lmfence();
    unsafe {
        asm!(
        "xchg {rbx_tmp}, rbx",
        ".byte 0x0f, 0x0f",
        "xchg {rbx_tmp}, rbx",
        inout("rax") address.address() => *res_a,
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
    if cfg!(feature = "emulation") {
        let low = (value & 0xFFFFFFFF) as u32;
        let high = (value >> 32) as u32;
        unsafe {
            asm!(
            "nop",
            in("ecx") msr,
            in("eax") low,
            in("edx") high,
            options(nostack, nomem, preserves_flags)
            );
        }
        return;
    }

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

// #[inline(always)]
pub fn activate_udebug_insts() {
    wrmsr(0x1e6, 0x200);
}

// #[inline(always)]
pub fn crbus_read(address: usize) -> usize {
    if cfg!(feature = "emulation") {
        // trace!("read CRBUS[{:08x}]", address);
        return 0;
    }

    core::hint::black_box(udebug_read(0, address))
}

// #[inline(always)]
pub fn crbus_write(address: usize, value: usize) -> usize {
    if cfg!(feature = "emulation") {
        // trace!("CRBUS[{:08x}] = {:08x}", address, value);
    }

    core::hint::black_box(udebug_write)(0, address, value);
    core::hint::black_box(udebug_read)(0, address)
}

#[inline(always)]
pub fn stgbuf_write_raw(address: usize, value: usize) {
    core::hint::black_box(udebug_write)(0x80, address, value)
}

#[inline(always)]
pub fn stgbuf_write(address: StagingBufferAddress, value: usize) {
    stgbuf_write_raw(address as usize, value)
}

//#[inline(always)]
pub fn stgbuf_read(address: usize) -> usize {
    core::hint::black_box(udebug_read(0x80, address))
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
    crbus_write(pdat_reg, fast_addr & 0xffff);
    crbus_write(pdat_reg + 4, val & 0xffffffff);
    crbus_write(pdat_reg + 5, (val >> 32) & 0xffff);
    crbus_write(pdat_reg + 1, 0);

    crbus_write(0x692, prev);
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FunctionResult {
    pub rax: usize,
    pub rbx: usize,
    pub rcx: usize,
    pub rdx: usize,
}

/// Calls to ucode. The target ucode is expected to take 3 arguments.
/// Provided to it in the registers `TMP0...TMP2`.
/// Output has to be stored in the registers `rax...rdx`.
/// The ucode function signals termination by SEQW LFNCEWAIT, UEND0
pub fn call_custom_ucode_function(
    func_address: UCInstructionAddress,
    args: [usize; 3],
) -> FunctionResult {
    let mut result = FunctionResult::default();

    stgbuf_write(RegTmp0, args[0]);
    stgbuf_write(RegTmp1, args[1]);
    stgbuf_write(RegTmp2, args[2]);

    core::hint::black_box(udebug_invoke)(
        func_address,
        &mut result.rax,
        &mut result.rbx,
        &mut result.rcx,
        &mut result.rdx,
    );

    stgbuf_write(RegTmp0, 0);
    stgbuf_write(RegTmp1, 0);
    stgbuf_write(RegTmp2, 0);

    result
}

fn ldat_array_read(
    ucode_read_function: UCInstructionAddress,
    pdat_reg: usize,
    array_sel: usize,
    bank_sel: usize,
    dword_idx: usize,
    fast_addr: usize,
) -> usize {
    // PSEUDO CODE
    // does not work when executing from outside of microcode
    // probably CRBUS reg are used in instruction decode or something
    //
    // let adr_selector: usize = pdat_reg + 1;
    // let adr_addr: usize = pdat_reg + 0;
    // let adr_value: usize = pdat_reg + 2;
    //
    // let _ = crbus_read(adr_selector);
    // crbus_write(
    //     adr_selector,
    //     0x10000 | ((dword_idx & 0xf) << 12) | ((array_sel & 0xf) << 8) | (bank_sel & 0xf),
    // );
    // crbus_write(adr_addr, 0xC00000 | (fast_addr & 0xffff));
    // let value = crbus_read(adr_value);
    // crbus_write(adr_selector, 0);

    let array_bank_sel =
        0x10000 | ((dword_idx & 0xf) << 12) | ((array_sel & 0xf) << 8) | (bank_sel & 0xf);
    let array_addr = 0xC00000 | (fast_addr & 0xffff);

    call_custom_ucode_function(ucode_read_function, [pdat_reg, array_bank_sel, array_addr]).rax
}

fn ms_array_write<A: MSRAMAddress>(
    array_sel: usize,
    bank_sel: usize,
    dword_idx: usize,
    fast_addr: A,
    val: usize,
) {
    ldat_array_write(
        0x6a0,
        array_sel,
        bank_sel,
        dword_idx,
        fast_addr.address(),
        val,
    )
}

fn ms_array_read<A: MSRAMAddress>(
    ucode_read_function: UCInstructionAddress,
    array_sel: usize,
    bank_sel: usize,
    dword_idx: usize,
    fast_addr: A,
) -> usize {
    ldat_array_read(
        ucode_read_function,
        0x6a0,
        array_sel,
        bank_sel,
        dword_idx,
        fast_addr.address(),
    )
}

pub fn ms_patch_instruction_write<A: Into<MSRAMInstructionPartWriteAddress>>(addr: A, val: usize) {
    let addr = addr.into();
    if cfg!(feature = "emulation") {
        trace!("Writing to MSRAM patch at {} = {:x}", addr, val);
    }
    ms_array_write(4, 0, 0, addr, val)
}

pub fn ms_patch_instruction_read<A: Into<MSRAMInstructionPartReadAddress>>(
    ucode_read_function: UCInstructionAddress,
    addr: A,
) -> usize {
    let addr = addr.into();
    if cfg!(feature = "emulation") {
        trace!("Reading from MSRAM at {:x}", addr.address());
    }
    ms_array_read(ucode_read_function, 4, 0, 0, addr)
}

pub fn ms_hook_write<A: Into<MSRAMHookIndex>>(addr: A, val: usize) {
    let addr = addr.into();
    if cfg!(feature = "emulation") {
        trace!("Writing to MSRAM hook at {:x} = {:x}", addr.address(), val);
    }
    ms_array_write(3, 0, 0, addr, val)
}

pub fn ms_hook_read<A: Into<MSRAMHookIndex>>(
    ucode_read_function: UCInstructionAddress,
    addr: A,
) -> usize {
    let addr = addr.into();
    if cfg!(feature = "emulation") {
        trace!("Reading from MSRAM hook at {:x}", addr.address());
    }
    ms_array_read(ucode_read_function, 3, 0, 0, addr)
}

pub fn ms_seqw_write<A: Into<MSRAMSequenceWordAddress>>(addr: A, val: usize) {
    let addr = addr.into();
    if cfg!(feature = "emulation") {
        trace!("Writing to MSRAM SEQW at {:x} = {:x}", addr.address(), val);
    }
    ms_array_write(2, 0, 0, addr, val)
}

pub fn ms_seqw_read<A: Into<MSRAMSequenceWordAddress>>(
    ucode_read_function: UCInstructionAddress,
    addr: A,
) -> usize {
    let addr = addr.into();
    if cfg!(feature = "emulation") {
        trace!("Reading from MSRAM SEQW at {:x}", addr.address());
    }
    ms_array_read(ucode_read_function, 2, 0, 0, addr)
}

pub fn detect_glm_version() -> u32 {
    CpuidResult::query(0x1, 0).eax
}

#[derive(Debug)]
pub enum PatchError {
    PatchToLarge,
}

pub fn patch_ucode<A: Into<UCInstructionAddress>>(
    addr: A,
    ucode_patch: &UcodePatchBlob,
) -> Result<(), PatchError> {
    // format: uop0, uop1, uop2, seqword
    // uop3 is fixed to a nop and cannot be overridden

    let addr = addr.into();

    if cfg!(feature = "emulation") {
        trace!("Writing ucode patch to {}", addr);
    }

    if (UCInstructionAddress::MAX - addr) < ucode_patch.len() * 4 {
        return Err(PatchError::PatchToLarge);
    }

    let seqw: MSRAMSequenceWordAddress = addr.into();

    for (i, row) in ucode_patch.iter().enumerate() {
        for (offset, entry) in row.iter().enumerate() {
            let addr = addr.patch_offset(i * 3 + offset);
            ms_patch_instruction_write(addr, *entry);
        }

        // patch seqword
        ms_seqw_write(seqw + i, row[3]);
    }

    Ok(())
}

pub fn read_patch(
    ucode_read_function: UCInstructionAddress,
    addr: UCInstructionAddress,
    ucode_patch: &mut UcodePatchBlob,
) {
    let seqw: MSRAMSequenceWordAddress = addr.into();

    for (i, row) in ucode_patch.iter_mut().enumerate() {
        for (offset, entry) in row.iter_mut().enumerate() {
            let read_val =
                ms_patch_instruction_read(ucode_read_function, addr.patch_offset(i * 3 + offset));
            *entry = read_val;
        }

        let read_val = ms_seqw_read(ucode_read_function, seqw + i);
        row[3] = read_val;
    }
}

pub fn calculate_hook_value(
    to_hook_ucode_addr: UCInstructionAddress,
    redirect_to_addr: UCInstructionAddress,
    enabled: bool,
) -> crate::Result<usize> {
    if !to_hook_ucode_addr.hookable() {
        return Err(Error::HookFailed(
            "patch uop address must be even and >0x7c00".to_string(),
        ));
    }
    let redirect_to_addr = redirect_to_addr.address();
    if redirect_to_addr < 0x7c00 {
        return Err(Error::HookFailed(
            "hook redirect address must be >0x7c00".to_string(),
        ));
    }

    let poff = (redirect_to_addr - 0x7c00) / 2;
    let patch_value =
        0x3e000000 | (poff << 16) | to_hook_ucode_addr.address() | if enabled { 1 } else { 0 };

    Ok(patch_value)
}

pub fn hook<A: Into<UCInstructionAddress>, B: Into<UCInstructionAddress>>(
    apply_hook_func: UCInstructionAddress,
    hook_idx: MSRAMHookIndex,
    to_hook_ucode_addr: A,
    redirect_to_addr: B,
    enabled: bool,
) -> crate::Result<()> {
    let patch_value =
        calculate_hook_value(to_hook_ucode_addr.into(), redirect_to_addr.into(), enabled)?;

    let result = call_custom_ucode_function(apply_hook_func, [patch_value, hook_idx.address(), 0]);

    if result.rax != 0x0000133700001337 && cfg!(not(feature = "emulation")) {
        return Err(Error::HookFailed(format!(
            "invoke({}) = {:016x}, {:016x}, {:016x}, {:016x}",
            apply_hook_func, result.rax, result.rbx, result.rcx, result.rdx
        )));
    }

    Ok(())
}

pub fn apply_patch(patch: &Patch) -> Result<(), PatchError> {
    patch_ucode(patch.addr, patch.ucode_patch)
}

pub fn apply_hook_patch_func() -> UCInstructionAddress {
    let patch = crate::patches::func_hook::PATCH;
    apply_patch(&patch).unwrap();
    patch.addr
}

pub fn apply_ldat_read_func() -> UCInstructionAddress {
    let patch = crate::patches::func_ldat_read::PATCH;
    apply_patch(&patch).unwrap();
    patch.addr
}

pub fn hook_patch(apply_hook_func: UCInstructionAddress, patch: &Patch) -> crate::Result<()> {
    if let Some(hook_address) = patch.hook_address {
        let hook_index = patch.hook_index.unwrap_or(MSRAMHookIndex::ZERO);

        hook(apply_hook_func, hook_index, hook_address, patch.addr, true)
    } else {
        Err(Error::HookFailed(
            "No hook address present in patch.".into(),
        ))
    }
}

pub fn enable_hooks() -> usize {
    let mp = crbus_read(0x692);
    crbus_write(0x692, mp & !1usize);
    mp
}

pub fn disable_all_hooks() -> usize {
    let mp = crbus_read(0x692);
    crbus_write(0x692, mp | 1usize);
    mp
}

pub fn restore_hooks(previous_value: usize) -> usize {
    let mp = crbus_read(0x692);
    crbus_write(0x692, (mp & !1) | (previous_value & 1));
    mp
}

pub struct HookGuard {
    previous_value: usize,
}

impl HookGuard {
    pub fn disable_all() -> Self {
        let previous_value = disable_all_hooks();
        HookGuard { previous_value }
    }

    pub fn enable_all() -> Self {
        let previous_value = enable_hooks();
        HookGuard { previous_value }
    }

    pub fn restore(self) {
        drop(self)
    }
}

impl Drop for HookGuard {
    fn drop(&mut self) {
        restore_hooks(self.previous_value);
    }
}

pub fn read_hook_status() -> usize {
    crbus_read(0x692)
}

fn read_ucode_clock() -> u64 {
    crbus_read(0x22d7) as u64
}

pub fn unwrap_ucode_clock(value: u64) -> u64 {
    (value & 0xffffffffffffff) * 0x39 + (value >> 0x37)
}

pub fn read_unwrap_ucode_clock() -> u64 {
    unwrap_ucode_clock(read_ucode_clock())
}
