//! x86-specific Performance Counter Implementation
//!
//! This module provides x86-specific implementations for working with performance counters.
//! It includes functionality for configuring and reading hardware performance monitoring events
//! using x86-specific instructions and MSRs.

use crate::PerfEventSpecifier;
use bitfield::bitfield;
use core::arch::asm;
use x86::cpuid::CpuId;
use x86::msr::{wrmsr, IA32_PERFEVTSEL0, IA32_PMC0};

bitfield! {
    /// Performance Event Select Register
    ///
    /// This bitfield struct represents the IA32_PERFEVTSELx MSR used to configure
    /// performance counters. It provides access to all fields of the register:
    ///
    /// - Event select (bits 0-7)
    /// - Unit mask (bits 8-15)
    /// - User mode flag (bit 16)
    /// - OS mode flag (bit 17)
    /// - Edge detect flag (bit 18)
    /// - Pin control flag (bit 19)
    /// - APIC interrupt flag (bit 20)
    /// - Enable counters flag (bit 22)
    /// - Invert flag (bit 23)
    /// - Counter mask (bits 24-31)
    ///
    /// See the Intel manual for more details.
    #[derive(Clone, Copy)]
    pub struct PerfEventSelect(u64);

    /// Event select field (bits 0 through 7) — Selects the event logic unit used to detect microarchitectural
    /// conditions. Each value corresponds to an event logic unit for use with an architectural performance event.
    pub event_select, set_event_select: 7, 0;

    /// Unit mask (UMASK) field (bits 8 through 15) — Qualifies the condition that the selected event logic unit detects.
    pub umask, set_umask: 15, 8;

    /// USR (user mode) flag (bit 16) — Specifies counting when the processor operates at privilege levels 1, 2, or 3.
    pub user_mode, set_user_mode: 16;

    /// OS (operating system mode) flag (bit 17) — Specifies counting when the processor operates at privilege level 0.
    pub os_mode, set_os_mode: 17;

    /// E (edge detect) flag (bit 18) — Enables edge detection of the selected microarchitectural condition.
    pub edge_detect, set_edge_detect: 18;

    /// PC (pin control) flag (bit 19) — Reserved in newer microarchitectures; toggles PMi pins in older ones.
    pub pin_control, set_pin_control: 19;

    /// INT (APIC interrupt enable) flag (bit 20) — Generates an exception through the local APIC on counter overflow.
    pub apic_interrupt, set_apic_interrupt: 20;

    /// EN (Enable Counters) Flag (bit 22) — Enables or disables performance counting in the corresponding counter.
    pub enable_counters, set_enable_counters: 22;

    /// INV (invert) flag (bit 23) — Inverts the counter-mask comparison for greater than or less than comparisons.
    pub invert, set_invert: 23;

    /// Counter mask (CMASK) field (bits 24 through 31) — Compares this mask to the event count during a single cycle.
    pub counter_mask, set_counter_mask: 31, 24;
}

impl PerfEventSelect {
    /// Applies a performance event specifier to this event select register
    ///
    /// # Arguments
    ///
    /// * `event` - The performance event specifier to apply
    ///
    /// # Returns
    ///
    /// Returns a mutable reference to self for chaining
    pub fn apply_perf_event_specifier<A: AsRef<PerfEventSpecifier>>(
        &mut self,
        event: A,
    ) -> &mut Self {
        let event = event.as_ref();
        self.set_event_select(event.event_select as u64);
        self.set_umask(event.umask as u64);
        if let Some(edge_detect) = event.edge_detect {
            self.set_edge_detect(edge_detect != 0);
        }
        if let Some(cmask) = event.cmask {
            self.set_counter_mask(cmask as u64);
        }
        self
    }

    /// Applies this event select register to the specified performance counter MSR
    ///
    /// # Arguments
    ///
    /// * `counter_index` - The index of the performance counter to configure
    ///
    /// # Safety
    ///
    /// This function is unsafe because it writes to MSRs, which requires privileged access.
    /// Further, the system may not support the pmc index.
    #[inline(always)]
    pub unsafe fn apply_to_msr(&self, counter_index: u8) {
        unsafe {
            wrmsr(IA32_PERFEVTSEL0 + counter_index as u32, self.0);
        }
    }
}

/// Writes a value to a performance counter MSR
///
/// # Arguments
///
/// * `counter_index` - The index of the performance counter to write to
/// * `value` - The value to write
///
/// # Safety
///
/// This function is unsafe because it writes to MSRs, which requires privileged access.
/// Further, the system may not support the pmc index.
pub unsafe fn write_pmc(counter_index: u8, value: u64) {
    unsafe {
        wrmsr(IA32_PMC0 + counter_index as u32, value);
    }
}

/// Reads the current value of a performance counter
///
/// # Arguments
///
/// * `counter_index` - The index of the performance counter to read
///
/// # Returns
///
/// Returns the current value of the performance counter
///
/// # Safety
///
/// This function is unsafe because it reads from MSRs, which requires privileged access.
/// Further, the system may not support the pmc index.
#[inline(always)]
pub unsafe fn read_pmc(counter_index: u8) -> u64 {
    let mut value: u64;
    unsafe {
        asm!(
        "rdpmc",
        "mov {value:e}, edx",
        "shl {value}, 32",
        "mov {value:e}, eax",
        in("ecx") counter_index as u32,
        out("rax") _,
        out("rdx") _,
        value = out(reg) value,
        options(nomem, nostack)
        );
    }
    value
}

/// Represents a hardware performance counter
///
/// This struct provides a high-level interface for working with x86 performance counters.
/// It handles configuration, enabling/disabling, and reading/writing counter values.
pub struct PerformanceCounter {
    /// The index of this performance counter
    pub index: u8,
    /// The event select register configuration for this counter
    pub perf_event: PerfEventSelect,
}

impl PerformanceCounter {
    /// Gets the number of available performance counters on the current CPU
    ///
    /// # Returns
    ///
    /// Returns the number of available performance counters
    ///
    /// # Panics
    ///
    /// Panics if performance counters are not available on the CPU
    pub fn number_of_counters() -> u8 {
        CpuId::new()
            .get_performance_monitoring_info()
            .expect("perf counters not available")
            .number_of_counters()
    }

    /// Creates a new performance counter configured with the specified event
    ///
    /// # Arguments
    ///
    /// * `index` - The index of the performance counter to use
    /// * `event` - The performance event to configure the counter with
    ///
    /// # Returns
    ///
    /// Returns a new PerformanceCounter instance
    /// 
    /// # Safety
    /// 
    /// Only one instance for each index may be used at the same time.
    pub unsafe fn from_perf_event_specifier(index: u8, event: &PerfEventSpecifier) -> Self {
        let mut result = Self::new(index);
        result.event().apply_perf_event_specifier(event);
        result.event().set_user_mode(true);
        result.event().set_os_mode(true);
        result.reset();

        result
    }

    /// Creates a new unconfigured performance counter
    ///
    /// # Arguments
    ///
    /// * `index` - The index of the performance counter to use
    ///
    /// # Returns
    ///
    /// Returns a new PerformanceCounter instance
    ///
    /// # Panics
    ///
    /// Panics if the specified index is greater than or equal to the number of available counters
    ///
    /// # Safety
    ///
    /// Only one instance of this struct should be created for a given index.
    pub unsafe fn new(index: u8) -> Self {
        if index >= Self::number_of_counters() {
            core::panic!(
                "Invalid performance counter index: {}. Max is {}",
                index,
                Self::number_of_counters()
            );
        }
        Self {
            index,
            perf_event: PerfEventSelect(0),
        }
    }

    /// Sets the event select register configuration for this counter
    ///
    /// # Arguments
    ///
    /// * `event` - The event select register configuration to use
    pub fn set_perf_event(&mut self, event: PerfEventSelect) {
        self.perf_event = event;
    }

    /// Enables this performance counter
    ///
    /// This function enables the counter and applies its current configuration.
    #[inline(always)]
    pub fn enable(&mut self) {
        unsafe {
            self.perf_event.set_enable_counters(true);
            self.perf_event.apply_to_msr(self.index);
        }
    }

    /// Disables this performance counter
    ///
    /// This function disables the counter by writing 0 to its event register.
    #[inline(always)]
    pub fn disable(&mut self) {
        unsafe {
            wrmsr(IA32_PERFEVTSEL0 + self.index as u32, 0);
        }
    }

    /// Gets a mutable reference to this counter's event select register
    ///
    /// # Returns
    ///
    /// Returns a mutable reference to the event select register
    pub fn event(&mut self) -> &mut PerfEventSelect {
        &mut self.perf_event
    }

    /// Reads the current value of this performance counter
    ///
    /// # Returns
    ///
    /// Returns the current value of the counter
    pub fn read(&self) -> u64 {
        unsafe { read_pmc(self.index) }
    }

    /// Writes a value to this performance counter
    ///
    /// # Arguments
    ///
    /// * `value` - The value to write
    pub fn write(&mut self, value: u64) {
        unsafe { write_pmc(self.index, value) }
    }

    /// Resets this performance counter
    ///
    /// This function disables the counter and sets its value to 0.
    pub fn reset(&mut self) {
        self.disable();
        self.write(0);
    }

    /// Gets the MSR offset for this counter's event select register
    ///
    /// # Returns
    ///
    /// Returns the MSR offset for the event select register
    pub const fn wrmsr_offset(&self) -> u32 {
        IA32_PERFEVTSEL0 + self.index as u32
    }
}
