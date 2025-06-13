//! Helper functions and types for microcode operations
//!
//! This module provides low-level functionality for:
//! - CPU instruction execution
//! - Memory barriers and fences
//! - Microcode reading and writing
//! - Hook management
//! - Staging buffer operations
//! - Array operations (LDAT)

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

/// Represents addresses in the staging buffer
#[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum StagingBufferAddress {
    /// Temporary register 0
    RegTmp0,
    /// Temporary register 1
    RegTmp1,
    /// Temporary register 2
    RegTmp2,
    /// Raw address value
    Raw(u16),
    // todo: check further values
}

impl StagingBufferAddress {
    /// Converts the staging buffer address to its numeric representation
    pub fn to_address(self) -> usize {
        match self {
            StagingBufferAddress::RegTmp0 => 0xb800,
            StagingBufferAddress::RegTmp1 => 0xb840,
            StagingBufferAddress::RegTmp2 => 0xb880,
            StagingBufferAddress::Raw(addr) => addr as usize,
        }
    }
}

impl Address for StagingBufferAddress {
    fn address(&self) -> usize {
        self.to_address()
    }
}

impl Display for StagingBufferAddress {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Executes the mfence instruction
#[inline(always)]
#[allow(unused)]
pub fn mfence() {
    unsafe { asm!("mfence", options(nostack)) }
}

/// Executes the lfence instruction
#[inline(always)]
#[allow(unused)]
pub fn lfence() {
    unsafe { asm!("lfence", options(nostack)) }
}

/// Executes lfence then mfence instructions
#[inline(always)]
pub fn lmfence() {
    unsafe { asm!("lfence; mfence", options(nostack)) }
}

/// Executes a write-back and invalidate cache instruction
#[inline(always)]
#[allow(unused)]
fn wbinvd() {
    unsafe { asm!("wbinvd", options(nostack, preserves_flags)) }
}

/// Creates a serializing barrier using CPUID instruction
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

/// Reads from the microcode debug interface
///
/// # Arguments
///
/// * `command` - The debug command to execute
/// * `address` - The address to read from
///
/// # Returns
///
/// The value read from the debug interface
///
/// # Literature
/// [1] https://doi.org/10.1007/s11416-022-00438-x
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

/// Writes to the microcode debug interface
///
/// # Arguments
///
/// * `command` - The debug command to execute
/// * `address` - The address to write to
/// * `value` - The value to write
///
/// # Literature
/// [1] https://doi.org/10.1007/s11416-022-00438-x
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

/// Invokes a microcode function and returns its results
///
/// # Arguments
///
/// * `address` - The address of the function to invoke
/// * `res_a` - Mutable reference to store RAX result
/// * `res_b` - Mutable reference to store RBX result
/// * `res_c` - Mutable reference to store RCX result
/// * `res_d` - Mutable reference to store RDX result
///
/// # Literature
/// [1] https://doi.org/10.1007/s11416-022-00438-x
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

/// Writes to a Model Specific Register (MSR)
///
/// # Arguments
///
/// * `msr` - The MSR number to write to
/// * `value` - The value to write
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

/// Result of a CPUID instruction
#[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CpuidResult {
    /// EAX register value
    pub eax: u32,
    /// EBX register value
    pub ebx: u32,
    /// ECX register value
    pub ecx: u32,
    /// EDX register value
    pub edx: u32,
}

impl CpuidResult {
    /// Executes CPUID instruction with given leaf and sub-leaf
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

/// Activates microcode debug instructions
pub fn activate_udebug_insts() {
    wrmsr(0x1e6, 0x200);
}

/// Reads from the CRBUS at the specified address
pub fn crbus_read(address: usize) -> usize {
    if cfg!(feature = "emulation") {
        // trace!("read CRBUS[{:08x}]", address);
        return 0;
    }

    core::hint::black_box(udebug_read(0, address))
}

/// Writes to the CRBUS at the specified address
pub fn crbus_write(address: usize, value: usize) -> usize {
    if cfg!(feature = "emulation") {
        // trace!("CRBUS[{:08x}] = {:08x}", address, value);
    }

    core::hint::black_box(udebug_write)(0, address, value);
    core::hint::black_box(udebug_read)(0, address)
}

/// Writes to the staging buffer at a raw address
pub fn stgbuf_write_raw(address: usize, value: usize) {
    core::hint::black_box(udebug_write)(0x80, address, value)
}

/// Writes to the staging buffer using a [`StagingBufferAddress`]
pub fn stgbuf_write(address: StagingBufferAddress, value: usize) {
    stgbuf_write_raw(address.to_address(), value)
}

/// Reads from the staging buffer at a raw address
pub fn stgbuf_read_raw(address: usize) -> usize {
    core::hint::black_box(udebug_read(0x80, address))
}

/// Reads from the staging buffer using a [`StagingBufferAddress`]
pub fn stgbuf_read(address: StagingBufferAddress) -> usize {
    stgbuf_read_raw(address.to_address())
}

/// Writes to the LDAT array
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

/// Result of a microcode function call
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FunctionResult {
    /// RAX register value
    pub rax: usize,
    /// RBX register value
    pub rbx: usize,
    /// RCX register value
    pub rcx: usize,
    /// RDX register value
    pub rdx: usize,
}

/// Calls a custom microcode function
///
/// # Arguments
///
/// * `func_address` - Address of the function to call
/// * `args` - Array of 3 arguments to pass to the function
///
/// # Returns
///
/// The function's result in the [`FunctionResult`] struct
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

/// Reads from the LDAT array
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

/// Writes to the MS array
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

/// Reads from the MS array
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

/// Writes an instruction to the MS patch array
pub fn ms_patch_instruction_write<A: Into<MSRAMInstructionPartWriteAddress>>(addr: A, val: usize) {
    let addr = addr.into();
    if cfg!(feature = "emulation") {
        trace!("Writing to MSRAM patch at {} = {:x}", addr, val);
    }
    ms_array_write(4, 0, 0, addr, val)
}

/// Reads an instruction from the MS patch array
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

/// Writes to a hook in the MS array
pub fn ms_hook_write<A: Into<MSRAMHookIndex>>(addr: A, val: usize) {
    let addr = addr.into();
    if cfg!(feature = "emulation") {
        trace!("Writing to MSRAM hook at {:x} = {:x}", addr.address(), val);
    }
    ms_array_write(3, 0, 0, addr, val)
}

/// Reads from a hook in the MS array
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

/// Writes to a sequence word in the MS array
pub fn ms_seqw_write<A: Into<MSRAMSequenceWordAddress>>(addr: A, val: usize) {
    let addr = addr.into();
    if cfg!(feature = "emulation") {
        trace!("Writing to MSRAM SEQW at {:x} = {:x}", addr.address(), val);
    }
    ms_array_write(2, 0, 0, addr, val)
}

/// Reads from a sequence word in the MS array
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

/// Detects the GLM processor version
pub fn detect_glm_version() -> u32 {
    CpuidResult::query(0x1, 0).eax
}

/// Error type for patch operations
#[derive(Debug)]
pub enum PatchError {
    /// The patch is too large for the target location
    PatchToLarge,
}

/// Applies a microcode patch at the specified address
///
/// # Arguments
///
/// * `addr` - The address to apply the patch at
/// * `ucode_patch` - The patch data to apply
///
/// # Returns
///
/// - `Ok(())` if the patch was applied successfully
/// - `Err(PatchError)` if the patch could not be applied
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

/// Reads a patch from the specified address
///
/// # Arguments
///
/// * `ucode_read_function` - Function to use for reading
/// * `addr` - Address to read from
/// * `ucode_patch` - Buffer to store the read patch
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

/// Calculates the value for a hook
///
/// # Arguments
///
/// * `to_hook_ucode_addr` - Address to hook
/// * `redirect_to_addr` - Address to redirect to
/// * `enabled` - Whether the hook should be enabled
///
/// # Returns
///
/// - `Ok(usize)` with the calculated hook value
/// - `Err(Error)` if the calculation fails
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

/// Sets up a hook in the microcode
///
/// # Arguments
///
/// * `apply_hook_func` - Function to use for applying the hook
/// * `hook_idx` - Index of the hook to set up
/// * `to_hook_ucode_addr` - Address to hook
/// * `redirect_to_addr` - Address to redirect to
/// * `enabled` - Whether the hook should be enabled
///
/// # Returns
///
/// - `Ok(())` if the hook was set up successfully
/// - `Err(Error)` if the hook setup fails
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

/// Applies a patch to the microcode
pub fn apply_patch(patch: &Patch) -> Result<(), PatchError> {
    patch_ucode(patch.addr, patch.ucode_patch)
}

/// Returns the address of the hook patch function, that is uploaded to microcode RAM
pub fn apply_hook_patch_func() -> UCInstructionAddress {
    let patch = crate::patches::func_hook::PATCH;
    apply_patch(&patch).unwrap();
    patch.addr
}

/// Returns the address of the LDAT read function, that is uploaded to microcode RAM
pub fn apply_ldat_read_func() -> UCInstructionAddress {
    let patch = crate::patches::func_ldat_read::PATCH;
    apply_patch(&patch).unwrap();
    patch.addr
}

/// Hooks a patch using the specified function
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

/// Enables all hooks globally
pub fn enable_hooks() -> usize {
    let mp = crbus_read(0x692);
    crbus_write(0x692, mp & !1usize);
    mp
}

/// Disables all hooks globally
pub fn disable_all_hooks() -> usize {
    let mp = crbus_read(0x692);
    crbus_write(0x692, mp | 1usize);
    mp
}

/// Restores hooks to a previous state
pub fn restore_hooks(previous_value: usize) -> usize {
    let mp = crbus_read(0x692);
    crbus_write(0x692, (mp & !1) | (previous_value & 1));
    mp
}

/// Checks if hooks are currently enabled
pub fn hooks_enabled() -> bool {
    let mp = crbus_read(0x692);
    mp & 1 == 0
}

/// RAII guard for managing hook state
pub struct HookGuard {
    previous_value: usize,
}

impl HookGuard {
    /// Creates a new guard that disables all hooks
    pub fn disable_all() -> Self {
        let previous_value = disable_all_hooks();
        HookGuard { previous_value }
    }

    /// Creates a new guard that enables all hooks
    pub fn enable_all() -> Self {
        let previous_value = enable_hooks();
        HookGuard { previous_value }
    }

    /// Explicitly restores the previous hook state
    pub fn restore(self) {
        drop(self)
    }
}

impl Drop for HookGuard {
    fn drop(&mut self) {
        restore_hooks(self.previous_value);
    }
}

/// Reads the current hook status
pub fn read_hook_status() -> usize {
    crbus_read(0x692)
}

/// Reads the raw ucode clock value
fn read_ucode_clock() -> u64 {
    crbus_read(0x22d7) as u64
}

/// Unwraps a raw ucode clock value
pub fn unwrap_ucode_clock(value: u64) -> u64 {
    (value & 0xffffffffffffff) * 0x39 + (value >> 0x37)
}

/// Reads and unwraps the current ucode clock value
pub fn read_unwrap_ucode_clock() -> u64 {
    unwrap_ucode_clock(read_ucode_clock())
}
