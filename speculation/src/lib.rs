#![no_std]

use core::arch::asm;
use bitfield::bitfield;
use uefi::{println, Status};
use x86::cpuid::CpuId;
use x86::msr::{rdmsr, wrmsr, IA32_PERFEVTSEL0, IA32_PERFEVTSEL4, IA32_PMC0};

pub mod patches;




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
    pub fn apply_perf_event_specifier<A: AsRef<PerfEventSpecifier>>(&mut self, event: A) -> &mut Self {
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
        wrmsr(IA32_PERFEVTSEL0+counter_index as u32, self.0);
    }
}

pub unsafe fn write_pmc(counter_index: u8, value: u64) {
    wrmsr(IA32_PMC0+counter_index as u32, value);
}


#[inline(always)]
pub unsafe fn read_pmc(counter_index: u8) -> u64 {
    let mut value: u64 = 0;
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
    value
}

pub struct PerformanceCounter {
    pub index: u8,
    pub perf_event_select_enable: PerfEventSelect,
    pub perf_event_select_disable: PerfEventSelect,
}

impl PerformanceCounter {
    pub fn new(index: u8) -> Self {
        let number = CpuId::new().get_performance_monitoring_info().expect("perf counters not available").number_of_counters();
        if index >= number {
            panic!("Invalid performance counter index: {}. Max is {}", index, number);
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

#[derive(Clone, Copy)]
pub struct PerfEventSpecifier {
    pub event_select: u8,
    pub umask: u8,
    pub edge_detect: Option<u8>,
    pub cmask: Option<u8>,
}

impl AsRef<PerfEventSpecifier> for PerfEventSpecifier {
    fn as_ref(&self) -> &PerfEventSpecifier {
        &self
    }
}

/// This event counts the number of uops delivered to Instruction Decode Queue (IDQ)
/// from the MITE path. Counting includes uops that may "bypass" the IDQ. This also
/// means that uops are not being delivered from the Decode Stream Buffer (DSB).
pub const IDQ_MITE_UOPS: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0x79,
    umask: 0x04,
    edge_detect: None,
    cmask: None,
};

/// This event counts cycles during which uops are being delivered to Instruction Decode
/// Queue (IDQ) from the MITE path. Counting includes uops that may "bypass" the IDQ.
pub const IDQ_MITE_CYCLES: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0x79,
    umask: 0x04,
    edge_detect: None,
    cmask: Some(1),
};

/// This event counts the number of uops delivered to Instruction Decode Queue (IDQ)
/// from the Decode Stream Buffer (DSB) path. Counting includes uops that may "bypass" the IDQ.
pub const IDQ_DSB_UOPS: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0x79,
    umask: 0x08,
    edge_detect: None,
    cmask: None,
};

/// This event counts cycles during which uops are being delivered to Instruction Decode
/// Queue (IDQ) from the Decode Stream Buffer (DSB) path. Counting includes uops that
/// may "bypass" the IDQ.
pub const IDQ_DSB_CYCLES: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0x79,
    umask: 0x08,
    edge_detect: None,
    cmask: Some(1),
};

/// This event counts the number of uops initiated by Decode Stream Buffer (DSB) that
/// are being delivered to Instruction Decode Queue (IDQ) while the Microcode Sequencer
/// (MS) is busy. Counting includes uops that may "bypass" the IDQ.
pub const IDQ_MS_DSB_UOPS: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0x79,
    umask: 0x10,
    edge_detect: None,
    cmask: None,
};

/// This event counts cycles during which uops initiated by Decode Stream Buffer (DSB)
/// are being delivered to Instruction Decode Queue (IDQ) while the Microcode Sequencer
/// (MS) is busy. Counting includes uops that may "bypass" the IDQ.
pub const IDQ_MS_DSB_CYCLES: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0x79,
    umask: 0x10,
    edge_detect: None,
    cmask: Some(1),
};

/// This event counts the number of deliveries to Instruction Decode Queue (IDQ)
/// initiated by Decode Stream Buffer (DSB) while the Microcode Sequencer (MS) is busy.
/// Counting includes uops that may "bypass" the IDQ.
pub const IDQ_MS_DSB_OCCUR: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0x79,
    umask: 0x10,
    edge_detect: Some(1),
    cmask: Some(1),
};

/// This event counts the number of cycles 4 uops were delivered to Instruction Decode
/// Queue (IDQ) from the Decode Stream Buffer (DSB) path. Counting includes uops that
/// may "bypass" the IDQ.
pub const IDQ_ALL_DSB_CYCLES_4_UOPS: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0x79,
    umask: 0x18,
    edge_detect: None,
    cmask: Some(4),
};

/// This event counts the number of cycles uops were delivered to Instruction Decode
/// Queue (IDQ) from the Decode Stream Buffer (DSB) path. Counting includes uops that
/// may "bypass" the IDQ.
pub const IDQ_ALL_DSB_CYCLES_ANY_UOPS: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0x79,
    umask: 0x18,
    edge_detect: None,
    cmask: Some(1),
};

/// This event counts the number of uops initiated by MITE and delivered to Instruction
/// Decode Queue (IDQ) while the Microcode Sequencer (MS) is busy. Counting includes
/// uops that may "bypass" the IDQ.
pub const IDQ_MS_MITE_UOPS: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0x79,
    umask: 0x20,
    edge_detect: None,
    cmask: None,
};

/// This event counts the number of cycles 4 uops were delivered to Instruction Decode
/// Queue (IDQ) from the MITE path. Counting includes uops that may "bypass" the IDQ.
/// This also means that uops are not being delivered from the Decode Stream Buffer (DSB).
pub const IDQ_ALL_MITE_CYCLES_4_UOPS: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0x79,
    umask: 0x24,
    edge_detect: None,
    cmask: Some(4),
};

/// This event counts the number of cycles uops were delivered to Instruction Decode
/// Queue (IDQ) from the MITE path. Counting includes uops that may "bypass" the IDQ.
/// This also means that uops are not being delivered from the Decode Stream Buffer (DSB).
pub const IDQ_ALL_MITE_CYCLES_ANY_UOPS: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0x79,
    umask: 0x24,
    edge_detect: None,
    cmask: Some(1),
};

/// This event counts the total number of uops delivered to Instruction Decode Queue
/// (IDQ) while the Microcode Sequencer (MS) is busy. Counting includes uops that may
/// "bypass" the IDQ. Uops may be initiated by Decode Stream Buffer (DSB) or MITE.
pub const IDQ_MS_UOPS: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0x79,
    umask: 0x30,
    edge_detect: None,
    cmask: None,
};

/// This event counts cycles during which uops are being delivered to Instruction Decode
/// Queue (IDQ) while the Microcode Sequencer (MS) is busy. Counting includes uops that
/// may "bypass" the IDQ. Uops may be initiated by Decode Stream Buffer (DSB) or MITE.
pub const IDQ_MS_CYCLES: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0x79,
    umask: 0x30,
    edge_detect: None,
    cmask: Some(1),
};

/// Number of switches from DSB (Decode Stream Buffer) or MITE (legacy decode pipeline)
/// to the Microcode Sequencer.
pub const IDQ_MS_SWITCHES: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0x79,
    umask: 0x30,
    edge_detect: Some(1),
    cmask: Some(1),
};

/// This event counts the number of uops delivered to Instruction Decode Queue (IDQ)
/// from the MITE path. Counting includes uops that may "bypass" the IDQ. This also
/// means that uops are not being delivered from the Decode Stream Buffer (DSB).
pub const IDQ_MITE_ALL_UOPS: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0x79,
    umask: 0x3C,
    edge_detect: None,
    cmask: None,
};





/// This event counts the number of times the Microcode Sequencer (MS) starts
/// a flow of uops from the MSROM. It does not count every time a uop is read
/// from the MSROM. The most common case that this counts is when a micro-coded
/// instruction is encountered by the front end of the machine. Other cases
/// include when an instruction encounters a fault, trap, or microcode assist
/// of any sort that initiates a flow of uops. The event will count MS startups
/// for uops that are speculative, and subsequently cleared by branch mispredict
/// or a machine clear.
pub const MS_DECODED_MS_ENTRY: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0xE7,
    umask: 0x01,
    edge_detect: None,
    cmask: None,
};

/// This event counts when the last uop of a branch instruction retires, which
/// corrected a misprediction of the branch prediction hardware at execution time.
pub const BRANCH_MISSES_RETIRED: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0xC5,
    umask: 0x00,
    edge_detect: None,
    cmask: None,
};

/// This event counts when the last uop of an instruction retires.
pub const INSTRUCTIONS_RETIRED: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0xC0,
    umask: 0x00,
    edge_detect: None,
    cmask: None,
};

/// This event counts uops issued by the front end and allocated into the back
/// end of the machine. This event counts uops that retire as well as uops that
/// were speculatively executed but didn't retire. The sort of speculative uops
/// that might be counted includes, but is not limited to those uops issued in
/// the shadow of a miss-predicted branch, those uops that are inserted during
/// an assist (such as for a denormal floating point result), and (previously
/// allocated) uops that might be canceled during a machine clear.
pub const UOPS_ISSUED_ANY: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0x0E,
    umask: 0x00,
    edge_detect: None,
    cmask: None,
};

/// This event counts uops which retired. It is a precise event.
pub const UOPS_RETIRED_ANY: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0xC2,
    umask: 0x00,
    edge_detect: None,
    cmask: None,
};