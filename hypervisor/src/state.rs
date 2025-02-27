use alloc::boxed::Box;
use alloc::vec::Vec;
use core::fmt::Debug;
use core::hash::Hash;
#[allow(unused_imports)]
use core::mem::size_of;
use core::ops::{Deref, DerefMut};
use core::{fmt, ptr};
use serde::{Deserialize, Serialize};
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
use x86::dtables::DescriptorTablePointer;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct VmState {
    pub standard_registers: GuestRegisters,
    pub extended_registers: VmStateExtendedRegisters,
}

/// The collection of the guest general purpose register values.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[repr(C)]
pub struct GuestRegisters {
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rbp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rip: u64,
    pub rsp: u64,
    pub rflags: u64,
    pub xmm0: M128A,
    pub xmm1: M128A,
    pub xmm2: M128A,
    pub xmm3: M128A,
    pub xmm4: M128A,
    pub xmm5: M128A,
    pub xmm6: M128A,
    pub xmm7: M128A,
    pub xmm8: M128A,
    pub xmm9: M128A,
    pub xmm10: M128A,
    pub xmm11: M128A,
    pub xmm12: M128A,
    pub xmm13: M128A,
    pub xmm14: M128A,
    pub xmm15: M128A,
}

impl GuestRegisters {
    // dont compare rip
    pub fn is_equal_no_address_compare(&self, other: &Self) -> bool {
        self.rax == other.rax
            && self.rbx == other.rbx
            && self.rcx == other.rcx
            && self.rdx == other.rdx
            && self.rdi == other.rdi
            && self.rsi == other.rsi
            && self.rbp == other.rbp
            && self.r8 == other.r8
            && self.r9 == other.r9
            && self.r10 == other.r10
            && self.r11 == other.r11
            && self.r12 == other.r12
            && self.r13 == other.r13
            && self.r14 == other.r14
            && self.r15 == other.r15
            && self.rsp == other.rsp
            && self.rflags == other.rflags
            && self.xmm0 == other.xmm0
            && self.xmm1 == other.xmm1
            && self.xmm2 == other.xmm2
            && self.xmm3 == other.xmm3
            && self.xmm4 == other.xmm4
            && self.xmm5 == other.xmm5
            && self.xmm6 == other.xmm6
            && self.xmm7 == other.xmm7
            && self.xmm8 == other.xmm8
            && self.xmm9 == other.xmm9
            && self.xmm10 == other.xmm10
            && self.xmm11 == other.xmm11
            && self.xmm12 == other.xmm12
            && self.xmm13 == other.xmm13
            && self.xmm14 == other.xmm14
            && self.xmm15 == other.xmm15
    }
}

#[repr(C)]
#[repr(align(16))]
#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct M128A {
    pub low: u64,
    pub high: i64,
}

impl fmt::Debug for M128A {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({:#018x}, {:#018x})", self.low, self.high)
    }
}

/// 26.4.1 Guest Register State
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct VmStateExtendedRegisters {
    #[serde(skip_serializing)]
    pub gdtr: DescriptorTablePointerWrapper<u64>,
    #[serde(skip_serializing)]
    pub idtr: DescriptorTablePointerWrapper<u64>,
    pub ldtr_base: u64,
    pub ldtr: u16,
    pub es: u16,
    pub cs: u16,
    pub ss: u16,
    pub ds: u16,
    pub fs: u16,
    pub gs: u16,
    pub tr: u16,
    pub efer: u64,
    pub cr0: u64,
    pub cr3: u64,
    pub cr4: u64,
    pub fs_base: u64,
    pub gs_base: u64,
    pub tr_base: u64,
    pub sysenter_cs: u64,
    pub sysenter_esp: u64,
    pub sysenter_eip: u64,
    pub dr7: u64,
    pub es_base: u64,
    pub cs_base: u64,
    pub ss_base: u64,
    pub ds_base: u64,
}

#[derive(Default)]
#[repr(transparent)]
pub struct DescriptorTablePointerWrapper<T>(pub DescriptorTablePointer<T>);

impl<T> From<DescriptorTablePointer<T>> for DescriptorTablePointerWrapper<T> {
    fn from(dtp: DescriptorTablePointer<T>) -> Self {
        DescriptorTablePointerWrapper(dtp)
    }
}

impl<T> Deref for DescriptorTablePointerWrapper<T> {
    type Target = DescriptorTablePointer<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for DescriptorTablePointerWrapper<u64> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> Debug for DescriptorTablePointerWrapper<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

impl<T> PartialEq for DescriptorTablePointerWrapper<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0.limit == other.0.limit && self.0.base as usize == other.0.base as usize
    }
}

impl<T> Clone for DescriptorTablePointerWrapper<T> {
    fn clone(&self) -> Self {
        DescriptorTablePointerWrapper(DescriptorTablePointer {
            base: self.0.base,
            limit: self.0.limit,
        })
    }
}

impl<T> Eq for DescriptorTablePointerWrapper<T> {}

impl<'de, T> Deserialize<'de> for DescriptorTablePointerWrapper<T> {
    fn deserialize<D>(_deserializer: D) -> Result<DescriptorTablePointerWrapper<T>, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        Ok(DescriptorTablePointerWrapper(DescriptorTablePointer {
            base: ptr::null(),
            limit: 0,
        }))
    }
}

// we are not using *const on the remote side
unsafe impl Send for DescriptorTablePointerWrapper<u64> {}
unsafe impl Sync for DescriptorTablePointerWrapper<u64> {}

impl Hash for DescriptorTablePointerWrapper<u64> {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        (self.0.base as usize).hash(state);
    }
}

pub trait StateDifference {
    fn difference<'a, 'b>(
        &'a self,
        other: &'b Self,
    ) -> Vec<(&'static str, Box<&'a dyn Debug>, Box<&'b dyn Debug>)>;
}

impl StateDifference for VmState {
    fn difference<'a, 'b>(
        &'a self,
        other: &'b Self,
    ) -> Vec<(&'static str, Box<&'a dyn Debug>, Box<&'b dyn Debug>)> {
        let mut differences: Vec<(&'static str, Box<&'a dyn Debug>, Box<&'b dyn Debug>)> =
            Vec::new();

        // todo implement reflection etc

        // standard registers
        if self.standard_registers != other.standard_registers {
            let this = &self.standard_registers;
            let other = &other.standard_registers;

            if this.rax != other.rax {
                differences.push(("rax", Box::new(&this.rax), Box::new(&other.rax)));
            }
            if this.rbx != other.rbx {
                differences.push(("rbx", Box::new(&this.rbx), Box::new(&other.rbx)));
            }
            if this.rcx != other.rcx {
                differences.push(("rcx", Box::new(&this.rcx), Box::new(&other.rcx)));
            }
            if this.rdx != other.rdx {
                differences.push(("rdx", Box::new(&this.rdx), Box::new(&other.rdx)));
            }
            if this.rsi != other.rsi {
                differences.push(("rsi", Box::new(&this.rsi), Box::new(&other.rsi)));
            }
            if this.rdi != other.rdi {
                differences.push(("rdi", Box::new(&this.rdi), Box::new(&other.rdi)));
            }
            if this.rbp != other.rbp {
                differences.push(("rbp", Box::new(&this.rbp), Box::new(&other.rbp)));
            }
            if this.rsp != other.rsp {
                differences.push(("rsp", Box::new(&this.rsp), Box::new(&other.rsp)));
            }
            if this.r8 != other.r8 {
                differences.push(("r8", Box::new(&this.r8), Box::new(&other.r8)));
            }
            if this.r9 != other.r9 {
                differences.push(("r9", Box::new(&this.r9), Box::new(&other.r9)));
            }
            if this.r10 != other.r10 {
                differences.push(("r10", Box::new(&this.r10), Box::new(&other.r10)));
            }
            if this.r11 != other.r11 {
                differences.push(("r11", Box::new(&this.r11), Box::new(&other.r11)));
            }
            if this.r12 != other.r12 {
                differences.push(("r12", Box::new(&this.r12), Box::new(&other.r12)));
            }
            if this.r13 != other.r13 {
                differences.push(("r13", Box::new(&this.r13), Box::new(&other.r13)));
            }
            if this.r14 != other.r14 {
                differences.push(("r14", Box::new(&this.r14), Box::new(&other.r14)));
            }
            if this.r15 != other.r15 {
                differences.push(("r15", Box::new(&this.r15), Box::new(&other.r15)));
            }
            if this.rip != other.rip {
                differences.push(("rip", Box::new(&this.rip), Box::new(&other.rip)));
            }
            if this.rflags != other.rflags {
                differences.push(("rflags", Box::new(&this.rflags), Box::new(&other.rflags)));
            }
            if this.xmm0 != other.xmm0 {
                differences.push(("xmm0", Box::new(&this.xmm0), Box::new(&other.xmm0)));
            }
            if this.xmm1 != other.xmm1 {
                differences.push(("xmm1", Box::new(&this.xmm1), Box::new(&other.xmm1)));
            }
            if this.xmm2 != other.xmm2 {
                differences.push(("xmm2", Box::new(&this.xmm2), Box::new(&other.xmm2)));
            }
            if this.xmm3 != other.xmm3 {
                differences.push(("xmm3", Box::new(&this.xmm3), Box::new(&other.xmm3)));
            }
            if this.xmm4 != other.xmm4 {
                differences.push(("xmm4", Box::new(&this.xmm4), Box::new(&other.xmm4)));
            }
            if this.xmm5 != other.xmm5 {
                differences.push(("xmm5", Box::new(&this.xmm5), Box::new(&other.xmm5)));
            }
            if this.xmm6 != other.xmm6 {
                differences.push(("xmm6", Box::new(&this.xmm6), Box::new(&other.xmm6)));
            }
            if this.xmm7 != other.xmm7 {
                differences.push(("xmm7", Box::new(&this.xmm7), Box::new(&other.xmm7)));
            }
            if this.xmm8 != other.xmm8 {
                differences.push(("xmm8", Box::new(&this.xmm8), Box::new(&other.xmm8)));
            }
            if this.xmm9 != other.xmm9 {
                differences.push(("xmm9", Box::new(&this.xmm9), Box::new(&other.xmm9)));
            }
            if this.xmm10 != other.xmm10 {
                differences.push(("xmm10", Box::new(&this.xmm10), Box::new(&other.xmm10)));
            }
            if this.xmm11 != other.xmm11 {
                differences.push(("xmm11", Box::new(&this.xmm11), Box::new(&other.xmm11)));
            }
            if this.xmm12 != other.xmm12 {
                differences.push(("xmm12", Box::new(&this.xmm12), Box::new(&other.xmm12)));
            }
            if this.xmm13 != other.xmm13 {
                differences.push(("xmm13", Box::new(&this.xmm13), Box::new(&other.xmm13)));
            }
            if this.xmm14 != other.xmm14 {
                differences.push(("xmm14", Box::new(&this.xmm14), Box::new(&other.xmm14)));
            }
            if this.xmm15 != other.xmm15 {
                differences.push(("xmm15", Box::new(&this.xmm15), Box::new(&other.xmm15)));
            }
        }

        if self.extended_registers != other.extended_registers {
            let this = &self.extended_registers;
            let other = &other.extended_registers;

            if this.gdtr != other.gdtr {
                differences.push(("gdtr", Box::new(&this.gdtr), Box::new(&other.gdtr)));
            }
            if this.idtr != other.idtr {
                differences.push(("idtr", Box::new(&this.idtr), Box::new(&other.idtr)));
            }
            if this.ldtr_base != other.ldtr_base {
                differences.push((
                    "ldtr_base",
                    Box::new(&this.ldtr_base),
                    Box::new(&other.ldtr_base),
                ));
            }
            if this.ldtr != other.ldtr {
                differences.push(("ldtr", Box::new(&this.ldtr), Box::new(&other.ldtr)));
            }
            if this.es != other.es {
                differences.push(("es", Box::new(&this.es), Box::new(&other.es)));
            }
            if this.cs != other.cs {
                differences.push(("cs", Box::new(&this.cs), Box::new(&other.cs)));
            }
            if this.ss != other.ss {
                differences.push(("ss", Box::new(&this.ss), Box::new(&other.ss)));
            }
            if this.ds != other.ds {
                differences.push(("ds", Box::new(&this.ds), Box::new(&other.ds)));
            }
            if this.fs != other.fs {
                differences.push(("fs", Box::new(&this.fs), Box::new(&other.fs)));
            }
            if this.gs != other.gs {
                differences.push(("gs", Box::new(&this.gs), Box::new(&other.gs)));
            }
            if this.tr != other.tr {
                differences.push(("tr", Box::new(&this.tr), Box::new(&other.tr)));
            }
            if this.efer != other.efer {
                differences.push(("efer", Box::new(&this.efer), Box::new(&other.efer)));
            }
            if this.cr0 != other.cr0 {
                differences.push(("cr0", Box::new(&this.cr0), Box::new(&other.cr0)));
            }
            if this.cr3 != other.cr3 {
                differences.push(("cr3", Box::new(&this.cr3), Box::new(&other.cr3)));
            }
            if this.cr4 != other.cr4 {
                differences.push(("cr4", Box::new(&this.cr4), Box::new(&other.cr4)));
            }
            if this.fs_base != other.fs_base {
                differences.push(("fs_base", Box::new(&this.fs_base), Box::new(&other.fs_base)));
            }
            if this.gs_base != other.gs_base {
                differences.push(("gs_base", Box::new(&this.gs_base), Box::new(&other.gs_base)));
            }
            if this.tr_base != other.tr_base {
                differences.push(("tr_base", Box::new(&this.tr_base), Box::new(&other.tr_base)));
            }
            if this.sysenter_cs != other.sysenter_cs {
                differences.push((
                    "sysenter_cs",
                    Box::new(&this.sysenter_cs),
                    Box::new(&other.sysenter_cs),
                ));
            }
            if this.sysenter_esp != other.sysenter_esp {
                differences.push((
                    "sysenter_esp",
                    Box::new(&this.sysenter_esp),
                    Box::new(&other.sysenter_esp),
                ));
            }
            if this.sysenter_eip != other.sysenter_eip {
                differences.push((
                    "sysenter_eip",
                    Box::new(&this.sysenter_eip),
                    Box::new(&other.sysenter_eip),
                ));
            }
            if this.dr7 != other.dr7 {
                differences.push(("dr7", Box::new(&this.dr7), Box::new(&other.dr7)));
            }
            if this.es_base != other.es_base {
                differences.push(("es_base", Box::new(&this.es_base), Box::new(&other.es_base)));
            }
            if this.cs_base != other.cs_base {
                differences.push(("cs_base", Box::new(&this.cs_base), Box::new(&other.cs_base)));
            }
            if this.ss_base != other.ss_base {
                differences.push(("ss_base", Box::new(&this.ss_base), Box::new(&other.ss_base)));
            }
            if this.ds_base != other.ds_base {
                differences.push(("ds_base", Box::new(&this.ds_base), Box::new(&other.ds_base)));
            }
        }

        differences
    }
}

impl VmState {
    pub fn null_addresses(mut self) -> VmState {
        self.standard_registers.rip = 0;
        self
    }

    pub fn is_equal_no_address_compare(&self, other: &Self) -> bool {
        self.standard_registers
            .is_equal_no_address_compare(&other.standard_registers)
            && self.extended_registers == other.extended_registers
    }
}

/// Reasons of VM exit.
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize, Hash)]
pub enum VmExitReason {
    /// An address translation failure with nested paging. GPA->LA. Contains a guest
    /// physical address that failed translation and whether the access was
    /// write access.
    EPTPageFault(EPTPageFaultQualification),

    /// An exception happened. Contains an exception code.
    Exception(ExceptionQualification),

    Cpuid,

    Hlt,

    Io,

    Rdmsr,

    Wrmsr,

    Rdrand,

    Rdseed,

    Rdtsc,

    Cr8Write,

    IoWrite,

    MsrUse,

    ExternalInterrupt,

    /// The guest ran long enough to use up its time slice.
    TimerExpiration,

    /// The logical processor entered the shutdown state, eg, triple fault.
    Shutdown(u64),

    /// An unhandled VM exit happened. Contains a vendor specific VM exit code.
    Unexpected(u64),

    /// VM entry failure: reason, qualification
    VMEntryFailure(u32, u64),

    /// If an SMM exit occurred that has a higher priority than a pending MTF Exit
    MonitorTrap,
}

impl VmExitReason {
    pub fn null_addresses(mut self) -> VmExitReason {
        match self {
            VmExitReason::EPTPageFault(ref mut info) => {
                info.rip = 0;
                info.gpa = 0;
            }
            VmExitReason::Exception(ref mut info) => {
                info.rip = 0;
            }
            _ => {}
        }
        self
    }
    pub fn is_same_kind(&self, other: &Self) -> bool {
        match self {
            VmExitReason::MonitorTrap => matches!(other, VmExitReason::MonitorTrap),
            VmExitReason::Cpuid => matches!(other, VmExitReason::Cpuid),
            VmExitReason::Hlt => matches!(other, VmExitReason::Hlt),
            VmExitReason::Io => matches!(other, VmExitReason::Io),
            VmExitReason::Rdmsr => matches!(other, VmExitReason::Rdmsr),
            VmExitReason::Wrmsr => matches!(other, VmExitReason::Wrmsr),
            VmExitReason::Rdrand => matches!(other, VmExitReason::Rdrand),
            VmExitReason::Rdseed => matches!(other, VmExitReason::Rdseed),
            VmExitReason::Rdtsc => matches!(other, VmExitReason::Rdtsc),
            VmExitReason::Cr8Write => matches!(other, VmExitReason::Cr8Write),
            VmExitReason::IoWrite => matches!(other, VmExitReason::IoWrite),
            VmExitReason::MsrUse => matches!(other, VmExitReason::MsrUse),
            VmExitReason::ExternalInterrupt => matches!(other, VmExitReason::ExternalInterrupt),
            VmExitReason::TimerExpiration => matches!(other, VmExitReason::TimerExpiration),
            VmExitReason::Shutdown(a) => {
                if let VmExitReason::Shutdown(b) = other {
                    a == b
                } else {
                    false
                }
            }
            VmExitReason::EPTPageFault(info) => {
                if let VmExitReason::EPTPageFault(other_info) = other {
                    info.data_read == other_info.data_read
                        && info.data_write == other_info.data_write
                        && info.instruction_fetch == other_info.instruction_fetch
                        && info.was_readable == other_info.was_readable
                        && info.was_writable == other_info.was_writable
                        && info.was_executable_user == other_info.was_executable_user
                        && info.was_executable_supervisor == other_info.was_executable_supervisor
                        && info.nmi_unblocking == other_info.nmi_unblocking
                        && info.shadow_stack_access == other_info.shadow_stack_access
                        && info.guest_paging_verification == other_info.guest_paging_verification
                } else {
                    false
                }
            }
            VmExitReason::Exception(a) => {
                if let VmExitReason::Exception(b) = other {
                    a.exception_code == b.exception_code
                } else {
                    false
                }
            }
            VmExitReason::Unexpected(a) => {
                if let VmExitReason::Unexpected(b) = other {
                    a == b
                } else {
                    false
                }
            }
            VmExitReason::VMEntryFailure(a, b) => {
                if let VmExitReason::VMEntryFailure(c, d) = other {
                    a == c && b == d
                } else {
                    false
                }
            }
        }
    }
}

impl Default for VmExitReason {
    fn default() -> Self {
        VmExitReason::Unexpected(0xFFFF)
    }
}

/// Details of the cause of nested page fault.
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize, Hash)]
pub struct EPTPageFaultQualification {
    pub rip: usize,
    pub gpa: usize,
    pub data_read: bool,
    pub data_write: bool,
    pub instruction_fetch: bool,
    pub was_readable: bool,
    pub was_writable: bool,
    pub was_executable_user: bool,
    pub was_executable_supervisor: bool,
    pub gla_valid: bool,
    pub nmi_unblocking: bool,
    pub shadow_stack_access: bool,
    pub guest_paging_verification: bool,
}

#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize, Hash)]
pub struct ExceptionQualification {
    pub rip: u64,
    pub exception_code: GuestException,
}

/// The cause of guest exception.
#[derive(Clone, Copy, PartialEq, Debug, Eq, Serialize, Deserialize, Hash)]
pub enum GuestException {
    DivideError,
    DebugException,
    NMIInterrupt,
    BreakPoint,
    Overflow,
    BoundRangeExceeded,
    InvalidOpcode,
    DeviceNotAvailable,
    DoubleFault,
    CoprocessorSegmentOverrun,
    InvalidTSS,
    SegmentNotPresent,
    StackSegmentFault,
    GeneralProtection,
    PageFault,
    FloatingPointError,
    AlignmentCheck,
    MachineCheck,
    SIMDException,
    VirtualizationException,
    ControlProtection,
    Reserved(u8),
    User(u8),
}

// from x86 crate

/// A struct describing a pointer to a descriptor table (GDT / IDT).
/// This is in a format suitable for giving to 'lgdt' or 'lidt'.
#[cfg(not(any(target_arch = "x86_64", target_arch = "x86")))]
#[repr(C, packed)]
pub struct DescriptorTablePointer<Entry> {
    /// Size of the DT.
    pub limit: u16,
    /// Pointer to the memory region containing the DT.
    pub base: *const Entry,
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "x86")))]
impl<T> Default for DescriptorTablePointer<T> {
    fn default() -> DescriptorTablePointer<T> {
        DescriptorTablePointer {
            limit: 0,
            base: core::ptr::null(),
        }
    }
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "x86")))]
impl<T> DescriptorTablePointer<T> {
    pub fn new(tbl: &T) -> Self {
        // GDT, LDT, and IDT all expect the limit to be set to "one less".
        // See Intel 3a, Section 3.5.1 "Segment Descriptor Tables" and
        // Section 6.10 "Interrupt Descriptor Table (IDT)".
        let len = size_of::<T>() - 1;
        assert!(len < 0x10000);
        DescriptorTablePointer {
            base: tbl as *const T,
            limit: len as u16,
        }
    }

    pub fn new_from_slice(slice: &[T]) -> Self {
        // GDT, LDT, and IDT all expect the limit to be set to "one less".
        // See Intel 3a, Section 3.5.1 "Segment Descriptor Tables" and
        // Section 6.10 "Interrupt Descriptor Table (IDT)".
        let len = slice.len() * size_of::<T>() - 1;
        assert!(len < 0x10000);
        DescriptorTablePointer {
            base: slice.as_ptr(),
            limit: len as u16,
        }
    }
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "x86")))]
impl<T> Debug for DescriptorTablePointer<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "DescriptorTablePointer ({} {:?})", { self.limit }, {
            self.base
        })
    }
}
