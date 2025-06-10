//! The module containing vendor agnostic representation of HW VT
//! (hardware-assisted virtualization technology) related definitions.

//pub mod svm;
pub mod vmx;

use crate::state::{
    EPTPageFaultQualification, GuestException, GuestRegisters, VmExitReason, VmState,
};
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

    /// Disable HW VT on the current processor.
    fn disable(&mut self);

    /// Configures HW VT such as enabling nested paging and exception
    /// interception.
    fn initialize(&mut self, nested_pml4_addr: u64) -> Result<(), &'static str>;

    /// Load the guest state.
    fn load_state(&mut self, state: &VmState);

    /// Save state
    fn save_state(&self, state: &mut VmState);

    /// Executes the guest until it triggers VM exit. Runs the after execution callback at the first possible moment (volatile state is saved).
    fn run_with_callback(&mut self, after_execution: fn()) -> VmExitReason;

    /// Executes the guest until it triggers VM exit.
    fn run(&mut self) -> VmExitReason {
        self.run_with_callback(|| {})
    }

    /// Invalidates caches of the nested paging structures.
    fn invalidate_caches(&mut self);

    /// Gets a flag value to be set to nested paging structure entries for the
    /// given entry types (eg, permissions).
    fn nps_entry_flags(
        &self,
        entry_type: NestedPagingStructureEntryType,
    ) -> NestedPagingStructureEntryFlags;
    fn set_preemption_timer(&self, timeout_in_tsc: u64);

    fn enable_tracing(&mut self);

    fn disable_tracing(&mut self);

    fn registers(&self) -> &GuestRegisters;
}

impl EPTPageFaultQualification {
    fn from(qualification: u64, rip: usize, gpa: usize) -> Self {
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

impl From<u8> for GuestException {
    fn from(vector: u8) -> Self {
        match vector {
            irq::DIVIDE_ERROR_VECTOR => GuestException::DivideError,
            irq::DEBUG_VECTOR => GuestException::DebugException,
            irq::NONMASKABLE_INTERRUPT_VECTOR => GuestException::NMIInterrupt,
            irq::BREAKPOINT_VECTOR => GuestException::BreakPoint,
            irq::OVERFLOW_VECTOR => GuestException::Overflow,
            irq::BOUND_RANGE_EXCEEDED_VECTOR => GuestException::BoundRangeExceeded,
            irq::INVALID_OPCODE_VECTOR => GuestException::InvalidOpcode,
            irq::DEVICE_NOT_AVAILABLE_VECTOR => GuestException::DeviceNotAvailable,
            irq::DOUBLE_FAULT_VECTOR => GuestException::DoubleFault,
            irq::COPROCESSOR_SEGMENT_OVERRUN_VECTOR => GuestException::CoprocessorSegmentOverrun,
            irq::INVALID_TSS_VECTOR => GuestException::InvalidTSS,
            irq::SEGMENT_NOT_PRESENT_VECTOR => GuestException::SegmentNotPresent,
            irq::STACK_SEGEMENT_FAULT_VECTOR => GuestException::StackSegmentFault,
            irq::GENERAL_PROTECTION_FAULT_VECTOR => GuestException::GeneralProtection,
            irq::PAGE_FAULT_VECTOR => GuestException::PageFault,
            irq::X87_FPU_VECTOR => GuestException::FloatingPointError,
            irq::ALIGNMENT_CHECK_VECTOR => GuestException::AlignmentCheck,
            irq::MACHINE_CHECK_VECTOR => GuestException::MachineCheck,
            irq::SIMD_FLOATING_POINT_VECTOR => GuestException::SIMDException,
            irq::VIRTUALIZATION_VECTOR => GuestException::VirtualizationException,
            21 => GuestException::ControlProtection,
            x if x == 15 || (x >= 22 && x <= 31) => GuestException::Reserved(x),
            x => GuestException::User(x),
        }
    }
}

/// Permissions and memory types to be specified for nested paging structure
/// entries.
pub enum NestedPagingStructureEntryType {
    /// None
    None,

    /// Execute
    X,

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

/// A single nested paging structure.
///
/// This is a extended page table on Intel and a nested page table on AMD. The
/// details of the layout are not represented in this structure so that it may
/// be used for any the structures (PML4, PDPT, PD and PT) across platforms.
#[derive(Clone, Debug)]
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
    pub permission, set_permission: 2, 0;
    pub memory_type, set_memory_type: 5, 3;
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
