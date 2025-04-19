use crate::PerfEventSpecifier;
use bitfield::bitfield;
use core::arch::asm;
use x86::cpuid::CpuId;
use x86::msr::{wrmsr, IA32_PERFEVTSEL0, IA32_PMC0};

bitfield! {
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

    #[inline(always)]
    pub unsafe fn apply_to_msr(&self, counter_index: u8) {
        unsafe {
            wrmsr(IA32_PERFEVTSEL0 + counter_index as u32, self.0);
        }
    }
}

pub unsafe fn write_pmc(counter_index: u8, value: u64) {
    unsafe {
        wrmsr(IA32_PMC0 + counter_index as u32, value);
    }
}

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

pub struct PerformanceCounter {
    pub index: u8,
    pub perf_event_select_enable: PerfEventSelect,
    pub perf_event_select_disable: PerfEventSelect,
}

impl PerformanceCounter {
    pub fn number_of_counters() -> u8 {
        CpuId::new()
            .get_performance_monitoring_info()
            .expect("perf counters not available")
            .number_of_counters()
    }

    pub fn new(index: u8) -> Self {
        if index >= Self::number_of_counters() {
            core::panic!(
                "Invalid performance counter index: {}. Max is {}",
                index,
                Self::number_of_counters()
            );
        }
        Self {
            index,
            perf_event_select_enable: PerfEventSelect(0),
            perf_event_select_disable: PerfEventSelect(0),
        }
    }

    pub fn set_perf_event(&mut self, event: PerfEventSelect) {
        self.perf_event_select_enable = event;
    }

    #[inline(always)]
    pub fn enable(&mut self) {
        unsafe {
            self.perf_event_select_enable.set_enable_counters(true);
            self.perf_event_select_disable.set_enable_counters(false);

            self.perf_event_select_enable.apply_to_msr(self.index);
        }
    }

    #[inline(always)]
    pub fn disable(&mut self) {
        unsafe {
            self.perf_event_select_disable.apply_to_msr(self.index);
        }
    }

    pub fn event(&mut self) -> &mut PerfEventSelect {
        &mut self.perf_event_select_enable
    }

    pub fn read(&self) -> u64 {
        unsafe { read_pmc(self.index) }
    }

    pub fn write(&mut self, value: u64) {
        unsafe { write_pmc(self.index, value) }
    }

    pub fn reset(&mut self) {
        self.disable();
        self.write(0);
    }
}
