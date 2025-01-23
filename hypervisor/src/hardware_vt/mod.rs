//! The module containing vendor agnostic representation of HW VT
//! (hardware-assisted virtualization technology) related definitions.

//pub mod svm;
pub mod vmx;

use crate::state::VmState;
use bitfield::bitfield;
use core::fmt;
use x86::{
    current::paging::{BASE_PAGE_SHIFT, PAGE_SIZE_ENTRIES},
    irq,
};

/// This trait represents an interface to enable HW VT, setup and run a single
/// virtual machine instance on the current processor.
pub trait HardwareVt: fmt::Debug {
    /// Enables HW VT on the current processor. It has to be called exactly once
    /// before calling any other method.
    fn enable(&mut self);

    /// Configures HW VT such as enabling nested paging and exception
    /// interception.
    fn initialize(&mut self, nested_pml4_addr: u64);

    /// Load the guest state.
    fn load_state(&mut self, state: &VmState);

    /// Save state
    fn save_state(&self, state: &mut VmState);

    /// Executes the guest until it triggers VM exit.
    fn run(&mut self) -> VmExitReason;

    /// Invalidates caches of the nested paging structures.
    fn invalidate_caches(&mut self);

    /// Gets a flag value to be set to nested paging structure entries for the
    /// given entry types (eg, permissions).
    fn nps_entry_flags(
        &self,
        entry_type: NestedPagingStructureEntryType,
    ) -> NestedPagingStructureEntryFlags;
    fn set_preemption_timer(&self, timeout_in_tsc: u64);
}

/// Reasons of VM exit.
#[derive(Debug)]
pub enum VmExitReason {
    /// An address translation failure with nested paging. GPA->LA. Contains a guest
    /// physical address that failed translation and whether the access was
    /// write access.
    EPTPageFault(EPTPageFaultQualification),

    /// An exception happened. Contains an exception code.
    Exception(ExceptionQualification),

    /// An external interrupt occurred, or `PAUSE` was executed more than
    /// certain times.
    ExternalInterruptOrPause,

    /// The guest ran long enough to use up its time slice.
    TimerExpiration,

    /// The logical processor entered the shutdown state, eg, triple fault.
    Shutdown(u64),

    /// An unhandled VM exit happened. Contains a vendor specific VM exit code.
    Unexpected(u64),
}

/// Details of the cause of nested page fault.
#[derive(Debug)]
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

impl EPTPageFaultQualification {
    pub fn from(qualification: u64, rip: usize, gpa: usize) -> Self {
        EPTPageFaultQualification {
            data_read: (qualification & (1 << 0)) != 0,
            data_write: (qualification & (1 << 1)) != 0,
            instruction_fetch: (qualification & (1 << 2)) != 0,
            was_readable: (qualification & (1 << 3)) != 0,
            was_writable: (qualification & (1 << 4)) != 0,
            was_executable_supervisor: (qualification & (1 << 5)) != 0,
            was_executable_user: (qualification & (1 << 6)) != 0,
            gla_valid: (qualification & (1 << 7)) != 0,
            nmi_unblocking: (qualification & (1 << 12)) != 0,
            shadow_stack_access: (qualification & (1 << 13)) != 0,
            guest_paging_verification: (qualification & (1 << 15)) != 0,
            rip,
            gpa,
        }
    }
}

#[derive(Debug)]
pub struct ExceptionQualification {
    pub rip: u64,
    pub exception_code: GuestException,
}

/// The cause of guest exception.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum GuestException {
    BreakPoint,
    InvalidOpcode,
    PageFault,
}

impl TryFrom<u8> for GuestException {
    type Error = &'static str;

    fn try_from(vector: u8) -> Result<Self, Self::Error> {
        match vector {
            irq::BREAKPOINT_VECTOR => Ok(GuestException::BreakPoint),
            irq::INVALID_OPCODE_VECTOR => Ok(GuestException::InvalidOpcode),
            irq::PAGE_FAULT_VECTOR => Ok(GuestException::PageFault),
            _ => Err("Vector of the exception that is not intercepted"),
        }
    }
}

/// Permissions and memory types to be specified for nested paging structure
/// entries.
pub enum NestedPagingStructureEntryType {
    /// None
    None,

    /// Read, execute
    Rx,

    /// Read only
    R,

    /// Read write
    Rw,

    /// Readable, writable, executable.
    Rwx,

    /// Readable, writable, executable, with the write-back memory type.
    RwxWriteBack,

    /// Readable, NON writable, executable, with the write-back memory type.
    RxWriteBack,
}

/// The values used to initialize [`NestedPagingStructureEntry`].
#[derive(Clone, Copy)]
pub struct NestedPagingStructureEntryFlags {
    pub permission: u8,
    pub memory_type: u8,
}

/// The collection of the guest general purpose register values.
#[derive(Debug, Default, Clone)]
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

#[repr(C)]
#[repr(align(16))]
#[derive(Clone, Copy, Default)]
pub struct M128A {
    pub low: u64,
    pub high: i64,
}

impl fmt::Debug for M128A {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({:#018x}, {:#018x})", self.low, self.high)
    }
}

/// A single nested paging structure.
///
/// This is a extended page table on Intel and a nested page table on AMD. The
/// details of the layout are not represented in this structure so that it may
/// be used for any the structures (PML4, PDPT, PD and PT) across platforms.
#[derive(Clone, Copy, Debug)]
#[repr(C, align(4096))]
pub struct NestedPagingStructure {
    /// An array of extended page table entry (8 bytes, 512 entries)
    pub entries: [NestedPagingStructureEntry; PAGE_SIZE_ENTRIES],
}
const _: () = assert!(size_of::<NestedPagingStructure>() == 0x1000);

bitfield! {
    /// Platform independent representation of a nested paging structure entry.
    ///
    /// Because it is platform independent, the layout is not exactly correct.
    /// For example, bit 5:3 `memory_type` exists only on Intel. On AMD, those are
    /// other bits and we set zeros.
    /*
         66665 5     1 110000 000 000
         32109 8.....2 109876 543 210
        +-----+-------+------+---+---+
        |xxxxx|  PFN  |xxxxxx| M | P |
        +-----+-------+------+---+---+
    */
    #[derive(Clone, Copy)]
    pub struct NestedPagingStructureEntry(u64);
    impl Debug;
    permission, set_permission: 2, 0;
    memory_type, set_memory_type: 5, 3;
    flags1, _: 11, 6;
    pub pfn, set_pfn: 58, 12;
    flags2, _: 63, 59;
}

impl NestedPagingStructureEntry {
    /// Returns the next nested paging structures.
    pub fn next_table_mut(&mut self) -> &mut NestedPagingStructure {
        let next_table_addr = self.pfn() << BASE_PAGE_SHIFT;
        assert!(next_table_addr != 0);
        let next_table_ptr = next_table_addr as *mut NestedPagingStructure;
        unsafe { next_table_ptr.as_mut() }.unwrap()
    }

    /// Sets the address to the next nested paging structure or final physical
    /// address with permissions specified by `flags`.
    pub fn set_translation(&mut self, pa: u64, flags: NestedPagingStructureEntryFlags) {
        self.set_pfn(pa >> BASE_PAGE_SHIFT);
        self.set_permission(u64::from(flags.permission));
        self.set_memory_type(u64::from(flags.memory_type));
    }
}

/// Returns the segment descriptor casted as a 64bit integer for the given
/// selector.
fn get_segment_descriptor_value(table_base: u64, selector: u16) -> u64 {
    let sel = x86::segmentation::SegmentSelector::from_raw(selector);
    let descriptor_addr = table_base + u64::from(sel.index() * 8);
    let ptr = descriptor_addr as *const u64;
    unsafe { *ptr }
}

/// Returns the limit of the given segment.
fn get_segment_limit(table_base: u64, selector: u16) -> u32 {
    let sel = x86::segmentation::SegmentSelector::from_raw(selector);
    if sel.index() == 0 && (sel.bits() >> 2) == 0 {
        return 0; // unusable
    }
    let descriptor_value = get_segment_descriptor_value(table_base, selector);
    let limit_low = descriptor_value & 0xffff;
    let limit_high = (descriptor_value >> (32 + 16)) & 0xF;
    let mut limit = limit_low | (limit_high << 16);
    if ((descriptor_value >> (32 + 23)) & 0x01) != 0 {
        limit = ((limit + 1) << BASE_PAGE_SHIFT) - 1;
    }
    limit as u32
}
