//! Hardware Virtualization Technology Module
//!
//! This module provides vendor-agnostic abstractions for hardware-assisted
//! virtualization technologies (VT-x for Intel, SVM for AMD). It defines the
//! core interfaces and data structures needed to manage virtual machines at
//! the hardware level.

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

/// Interface for hardware-assisted virtualization
///
/// This trait defines the core functionality required to enable, configure,
/// and manage hardware virtualization on a processor. It provides methods for
/// enabling/disabling virtualization, managing VM state, and handling VM exits.
pub trait HardwareVt: fmt::Debug {
    /// Enables hardware virtualization on the current processor
    ///
    /// This method must be called exactly once before any other methods
    /// are used. It initializes the hardware virtualization features.
    fn enable(&mut self);

    /// Disables hardware virtualization on the current processor
    ///
    /// This method cleans up hardware virtualization resources and returns
    /// the processor to normal operation.
    fn disable(&mut self);

    /// Configures hardware virtualization features
    ///
    /// Sets up nested paging, exception interception, and other virtualization
    /// features. Must be called after `enable()` and before running a VM.
    ///
    /// # Arguments
    ///
    /// * `nested_pml4_addr` - The physical address of the nested PML4 table
    fn initialize(&mut self, nested_pml4_addr: u64) -> Result<(), &'static str>;

    /// Loads guest state into the hardware virtualization structures
    ///
    /// # Arguments
    ///
    /// * `state` - The VM state to load
    fn load_state(&mut self, state: &VmState);

    /// Saves the current guest state
    ///
    /// # Arguments
    ///
    /// * `state` - The VM state to save into
    fn save_state(&self, state: &mut VmState);

    /// Executes the guest VM with a callback
    ///
    /// Runs the guest until a VM exit occurs, then executes the provided
    /// callback function at the first possible moment after saving volatile state.
    ///
    /// # Arguments
    ///
    /// * `after_execution` - Function to call after VM execution
    ///
    /// # Returns
    ///
    /// The reason for the VM exit
    fn run_with_callback(&mut self, after_execution: fn()) -> VmExitReason;

    /// Executes the guest VM
    ///
    /// Runs the guest until a VM exit occurs.
    ///
    /// # Returns
    ///
    /// The reason for the VM exit
    fn run(&mut self) -> VmExitReason {
        self.run_with_callback(|| {})
    }

    /// Invalidates nested paging structure caches
    ///
    /// This method ensures that changes to page tables are visible to the
    /// hardware virtualization unit.
    fn invalidate_caches(&mut self);

    /// Gets flags for nested paging structure entries
    ///
    /// # Arguments
    ///
    /// * `entry_type` - The type of page table entry
    ///
    /// # Returns
    ///
    /// The flags to set in the page table entry
    fn nps_entry_flags(
        &self,
        entry_type: NestedPagingStructureEntryType,
    ) -> NestedPagingStructureEntryFlags;

    /// Sets the preemption timer
    ///
    /// # Arguments
    ///
    /// * `timeout_in_tsc` - Timeout value in TSC cycles
    fn set_preemption_timer(&self, timeout_in_tsc: u64);

    /// Enables tracing of VM execution
    fn enable_tracing(&mut self);

    /// Disables tracing of VM execution
    fn disable_tracing(&mut self);

    /// Returns a reference to the current guest registers
    fn registers(&self) -> &GuestRegisters;
}

impl EPTPageFaultQualification {
    /// Parse an EPT page fault qualification from raw values
    ///
    /// # Arguments
    ///
    /// * `qualification` - Raw qualification bits
    /// * `rip` - Instruction pointer at time of fault
    /// * `gpa` - Guest physical address that caused the fault
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

/// Page table entry types for nested paging
///
/// This enum defines the different access types of page table entries that can be
/// created in the nested paging structures, specifying their permissions
/// and memory types.
pub enum NestedPagingStructureEntryType {
    /// No permissions
    None,
    /// Execute only
    X,
    /// Read and execute
    Rx,
    /// Read only
    R,
    /// Read and write
    Rw,
    /// Read, write, and execute
    Rwx,
    /// Read, write, execute with write-back memory type
    RwxWriteBack,
    /// Read and execute with write-back memory type
    RxWriteBack,
}

/// Flags for nested paging structure entries
///
/// This structure contains the permission and memory type flags that are
/// used to initialize page table entries in the nested paging structures.
#[derive(Clone, Copy)]
pub struct NestedPagingStructureEntryFlags {
    /// Permission bits
    pub permission: u8,
    /// Memory type bits
    pub memory_type: u8,
}

/// A nested paging structure
///
/// This structure represents a page table in the nested paging hierarchy
/// (PML4, PDPT, PD, or PT). It is used for both Intel EPT and AMD NPT.
#[derive(Clone, Debug)]
#[repr(C, align(4096))]
pub struct NestedPagingStructure {
    /// Array of page table entries
    pub entries: [NestedPagingStructureEntry; PAGE_SIZE_ENTRIES],
}
const _: () = assert!(size_of::<NestedPagingStructure>() == 0x1000);

bitfield! {
    /// A nested paging structure entry
    ///
    /// This structure represents a single entry in a nested paging structure.
    /// It is platform-independent, so some bits may have different meanings
    /// on different architectures.
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
    /// Returns a mutable reference to the next level page table
    ///
    /// # Returns
    ///
    /// A mutable reference to the next level page table structure
    pub fn next_table_mut(&mut self) -> &mut NestedPagingStructure {
        let next_table_addr = self.pfn() << BASE_PAGE_SHIFT;
        assert!(next_table_addr != 0);
        let next_table_ptr = next_table_addr as *mut NestedPagingStructure;
        unsafe { next_table_ptr.as_mut() }.unwrap()
    }

    /// Sets the translation and flags for this entry
    ///
    /// # Arguments
    ///
    /// * `pa` - Physical address to translate to
    /// * `flags` - Permission and memory type flags
    pub fn set_translation(&mut self, pa: u64, flags: NestedPagingStructureEntryFlags) {
        self.set_pfn(pa >> BASE_PAGE_SHIFT);
        self.set_permission(u64::from(flags.permission));
        self.set_memory_type(u64::from(flags.memory_type));
    }
}

/// Gets the segment descriptor value for a selector
///
/// # Arguments
///
/// * `table_base` - Base address of the descriptor table
/// * `selector` - Segment selector
///
/// # Returns
///
/// The 64-bit segment descriptor value
fn get_segment_descriptor_value(table_base: u64, selector: u16) -> u64 {
    let sel = x86::segmentation::SegmentSelector::from_raw(selector);
    let descriptor_addr = table_base + u64::from(sel.index() * 8);
    let ptr = descriptor_addr as *const u64;
    unsafe { *ptr }
}

/// Gets the segment limit for a selector
///
/// # Arguments
///
/// * `table_base` - Base address of the descriptor table
/// * `selector` - Segment selector
///
/// # Returns
///
/// The segment limit in bytes
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
