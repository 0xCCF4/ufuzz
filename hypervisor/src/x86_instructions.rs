//! x86 Instruction Wrappers Module
//!
//! This module provides "safe" wrapper functions for x86 instructions that are
//! typically marked as unsafe in the `x86` crate. These wrappers encapsulate
//! the unsafe operations since this project always runs at CPL0 (ring 0) and
//! satisfies the necessary preconditions.
//!
//! # Safety
//!
//! All functions in this module are safe to call because:
//! 1. The hypervisor always runs at CPL0 (ring 0)
//! 2. The necessary preconditions for each instruction are always satisfied
//! 3. The operations are performed in a controlled environment

use core::arch::asm;
use x86::{
    controlregs::{Cr0, Cr4},
    dtables::DescriptorTablePointer,
};

/// Reads the current value of the Time Stamp Counter (TSC)
pub fn rdtsc() -> u64 {
    // Safety: this project runs at CPL0.
    unsafe { core::arch::x86_64::_rdtsc() }
}

/// Reads a Model-Specific Register (MSR)
///
/// # Arguments
///
/// * `msr` - The MSR register number to read
///
/// # Returns
///
/// The 64-bit value stored in the specified MSR
pub fn rdmsr(msr: u32) -> u64 {
    // Safety: this project runs at CPL0.
    unsafe { x86::msr::rdmsr(msr) }
}

/// Writes a value to a Model-Specific Register (MSR)
///
/// # Arguments
///
/// * `msr` - The MSR register number to write to
/// * `value` - The 64-bit value to write
pub fn wrmsr(msr: u32, value: u64) {
    // Safety: this project runs at CPL0.
    unsafe { x86::msr::wrmsr(msr, value) };
}

/// Reads the CR0 control register
pub fn cr0() -> Cr0 {
    // Safety: this project runs at CPL0.
    unsafe { x86::controlregs::cr0() }
}

/// Writes a value to the CR0 control register
///
/// # Arguments
///
/// * `val` - The new value for CR0
pub fn cr0_write(val: Cr0) {
    // Safety: this project runs at CPL0.
    unsafe { x86::controlregs::cr0_write(val) };
}

/// Reads the CR3 control register
pub fn cr3() -> u64 {
    // Safety: this project runs at CPL0.
    unsafe { x86::controlregs::cr3() }
}

/// Reads the CR4 control register
pub fn cr4() -> Cr4 {
    // Safety: this project runs at CPL0.
    unsafe { x86::controlregs::cr4() }
}

/// Writes a value to the CR4 control register
///
/// # Arguments
///
/// * `val` - The new value for CR4
pub fn cr4_write(val: Cr4) {
    // Safety: this project runs at CPL0.
    unsafe { x86::controlregs::cr4_write(val) };
}

/// Disables maskable interrupts.
pub fn cli() {
    // Safety: this project runs at CPL0.
    unsafe { x86::irq::disable() };
}

/// Halts the processor.
pub fn hlt() {
    // Safety: this project runs at CPL0.
    unsafe { x86::halt() };
}

/// Reads a byte from an I/O port
///
/// # Arguments
///
/// * `port` - The I/O port number to read from
///
/// # Returns
///
/// The byte read from the specified port
pub fn inb(port: u16) -> u8 {
    // Safety: this project runs at CPL0.
    unsafe { x86::io::inb(port) }
}

/// Writes a byte to an I/O port
///
/// # Arguments
///
/// * `port` - The I/O port number to write to
/// * `val` - The byte value to write
pub fn outb(port: u16, val: u8) {
    // Safety: this project runs at CPL0.
    unsafe { x86::io::outb(port, val) };
}

/// Reads the Interrupt Descriptor Table Register (IDTR)
///
/// # Arguments
///
/// * `idtr` - A mutable reference to store the IDTR value
pub fn sidt<T>(idtr: &mut DescriptorTablePointer<T>) {
    // Safety: this project runs at CPL0.
    unsafe { x86::dtables::sidt(idtr) };
}

/// Reads the Global Descriptor Table Register (GDTR)
///
/// # Arguments
///
/// * `gdtr` - A mutable reference to store the GDTR value
pub fn sgdt<T>(gdtr: &mut DescriptorTablePointer<T>) {
    // Safety: this project runs at CPL0.
    unsafe { x86::dtables::sgdt(gdtr) };
}

/// Executes a Bochs magic breakpoint
///
/// This function generates a special instruction sequence that Bochs recognizes
/// as a breakpoint. It has no effect when running outside of Bochs.
#[allow(clippy::inline_always, clippy::doc_markdown, dead_code)]
#[inline(always)]
pub fn bochs_breakpoint() {
    unsafe { asm!("xchg %bx, %bx", options(att_syntax, nomem, nostack)) };
}
