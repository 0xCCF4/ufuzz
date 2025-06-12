//! # Coverage Interface
//! 
//! This module provides the interface for interacting with coverage data collection.
//! It contains both a raw unsafe interface and a safe UEFI-specific interface.
//! 
//! ## Raw Interface
//! 
//! The raw interface provides direct memory access to coverage data structures, while
//! the user is responsible for managing the memory region.
//! 
//! ## Safe Interface
//! 
//! The safe interface (available with the `uefi` feature) provides memory-safe
//! operations with proper page allocation and boundary checks.

#[allow(clippy::missing_safety_doc)] // todo: remove this and write safety docs
pub mod raw {
    use crate::interface_definition::{
        ComInterfaceDescription, CoverageEntry, InstructionTableEntry, JumpTableEntry, LastRIPEntry,
    };
    use core::ptr::NonNull;
    use data_types::addresses::{Address, UCInstructionAddress};

    /// Raw interface for direct memory access to coverage data
    pub struct ComInterface<'a> {
        /// Base pointer to the coverage data in memory
        base: NonNull<u8>,
        /// Description of the interface layout
        pub description: &'a ComInterfaceDescription,
    }

    impl<'a> ComInterface<'a> {
        /// Creates a new raw interface with the given description
        /// 
        /// # Safety
        /// 
        /// The caller must ensure that:
        /// - The base address in the description is valid
        /// - The memory region is properly allocated
        /// - No other part of the program is accessing this memory region
        pub unsafe fn new(description: &'a ComInterfaceDescription) -> Self {
            if description.base == 0 {
                panic!("Invalid base address");
            }

            let ptr = NonNull::new_unchecked(description.base as *mut u8);

            Self {
                base: ptr,
                description,
            }
        }

        /// Gets a pointer to the jump table
        unsafe fn jump_table(&self) -> NonNull<JumpTableEntry> {
            self.base
                .add(self.description.offset_jump_back_table)
                .cast()
        }

        /// Reads the entire jump table
        /// 
        /// # Safety
        /// 
        /// The caller must ensure that the memory region is valid and not being modified
        #[allow(unused)]
        pub unsafe fn read_jump_table(&self) -> &[JumpTableEntry] {
            core::slice::from_raw_parts(
                self.jump_table().as_ptr(),
                self.description.max_number_of_hooks,
            )
        }

        /// Writes a value to the jump table at the specified index
        /// 
        /// # Safety
        /// 
        /// The caller must ensure that the memory region is valid and not being read
        pub unsafe fn write_jump_table<P: Into<UCInstructionAddress>>(
            &mut self,
            index: usize,
            value: P,
        ) {
            if index >= self.description.max_number_of_hooks {
                return;
            }

            self.jump_table()
                .add(index)
                .write_volatile(value.into().address() as JumpTableEntry);
        }

        /// Writes multiple values to the jump table
        /// 
        /// # Safety
        /// 
        /// The caller must ensure that the memory region is valid and not being read
        pub unsafe fn write_jump_table_all<
            P: Into<UCInstructionAddress>,
            T: IntoIterator<Item = P>,
        >(
            &mut self,
            values: T,
        ) {
            for (index, value) in values.into_iter().enumerate() {
                self.write_jump_table(index, value);
            }
        }

        /// Zeros out the entire jump table
        /// 
        /// # Safety
        /// 
        /// The caller must ensure that the memory region is valid and not being read
        pub unsafe fn zero_jump_table(&mut self) {
            for i in 0..self.description.max_number_of_hooks {
                self.write_jump_table(i, UCInstructionAddress::ZERO);
            }
        }

        /// Gets a pointer to the coverage table
        unsafe fn coverage_table(&self) -> NonNull<CoverageEntry> {
            self.base
                .add(self.description.offset_coverage_result_table)
                .cast()
        }

        /// Reads a coverage entry at the specified index
        /// 
        /// # Safety
        /// 
        /// The caller must ensure that the memory region is valid and not being modified
        pub unsafe fn read_coverage_table(&self, index: usize) -> CoverageEntry {
            if index >= self.description.max_number_of_hooks {
                return CoverageEntry::default();
            }

            self.coverage_table().add(index).read_volatile()
        }

        /// Writes a coverage entry at the specified index
        /// 
        /// # Safety
        /// 
        /// The caller must ensure that the memory region is valid and not being read
        pub unsafe fn write_coverage_table(&mut self, index: usize, value: CoverageEntry) {
            if index >= self.description.max_number_of_hooks {
                return;
            }

            self.coverage_table().add(index).write_volatile(value);
        }

        /// Resets all coverage entries to their default values
        /// 
        /// # Safety
        /// 
        /// The caller must ensure that the memory region is valid and not being read
        pub unsafe fn reset_coverage(&mut self) {
            for index in 0..self.description.max_number_of_hooks * 2 {
                self.write_coverage_table(index, CoverageEntry::default());
            }
        }

        /// Gets a pointer to the instruction table
        unsafe fn instruction_table(&self) -> NonNull<InstructionTableEntry> {
            self.base
                .add(self.description.offset_instruction_table)
                .cast()
        }

        /// Reads an instruction entry at the specified index
        pub fn read_instruction_table(&self, index: usize) -> InstructionTableEntry {
            if index >= self.description.max_number_of_hooks {
                return InstructionTableEntry::default();
            }

            unsafe { self.instruction_table().add(index).read_volatile() }
        }

        /// Writes an instruction entry at the specified index
        pub fn write_instruction_table(&mut self, index: usize, value: InstructionTableEntry) {
            if index >= self.description.max_number_of_hooks {
                return;
            }

            unsafe { self.instruction_table().add(index).write_volatile(value) }
        }

        /// Writes multiple instruction entries
        pub unsafe fn write_instruction_table_all<
            P: Into<InstructionTableEntry>,
            T: IntoIterator<Item = P>,
        >(
            &mut self,
            values: T,
        ) {
            for (index, value) in values.into_iter().enumerate() {
                self.write_instruction_table(index, value.into());
            }
        }

        /// Gets a pointer to the last RIP table
        unsafe fn lastrip_table(&self) -> NonNull<LastRIPEntry> {
            self.base.add(self.description.offset_last_rip_table).cast()
        }

        /// Reads the last RIP entry at the specified index
        pub fn read_last_rip(&self, index: usize) -> LastRIPEntry {
            if index >= self.description.max_number_of_hooks {
                return LastRIPEntry::default();
            }

            unsafe { self.lastrip_table().add(index).read_volatile() }
        }
    }
}

#[cfg(feature = "uefi")]
pub mod safe {
    use crate::interface_definition::{
        ComInterfaceDescription, CoverageEntry, InstructionTableEntry, JumpTableEntry,
        LastRIPEntry, COVERAGE_RESULT_TABLE_BYTE_SIZE, INSTRUCTION_TABLE_BYTE_SIZE,
        JUMP_TABLE_BYTE_SIZE, LAST_RIP_TABLE_BYTE_SIZE,
    };
    use crate::page_allocation::PageAllocation;
    use data_types::addresses::UCInstructionAddress;
    use log::error;
    use uefi::data_types::PhysicalAddress;

    /// A safe interface for coverage data collection in UEFI environments
    /// 
    /// This interface provides memory-safe operations by managing page allocations
    /// and ensuring proper memory boundaries.
    pub struct ComInterface<'a> {
        /// The underlying raw interface
        base: super::raw::ComInterface<'a>,
        /// Page allocation for the coverage data
        #[allow(dead_code)]
        // this is required since, while PageAllocation is alive, the memory is reserved
        allocation: PageAllocation,
    }

    impl<'a> ComInterface<'a> {
        /// Creates a new safe interface with proper page allocation
        /// 
        /// # Errors
        /// 
        /// Returns an error if:
        /// - The base address is invalid (zero)
        /// - The memory layout has overlapping regions
        /// - Page allocation fails
        pub fn new(description: &'a ComInterfaceDescription) -> uefi::Result<Self> {
            if description.base == 0 {
                return Err(uefi::Status::INVALID_PARAMETER.into());
            }

            if !description.check_overlap() {
                error!("Overlap detected in ComInterfaceDescription");
                error!(
                    " - Coverage table    [{:04x} - {:04x}]",
                    description.offset_coverage_result_table,
                    description.offset_coverage_result_table + COVERAGE_RESULT_TABLE_BYTE_SIZE - 1
                );
                error!(
                    " - Jump table        [{:04x} - {:04x}]",
                    description.offset_jump_back_table,
                    description.offset_jump_back_table + JUMP_TABLE_BYTE_SIZE - 1
                );
                error!(
                    " - Instruction table [{:04x} - {:04x}]",
                    description.offset_instruction_table,
                    description.offset_instruction_table + INSTRUCTION_TABLE_BYTE_SIZE - 1
                );
                error!(
                    " - lastRIP table [{:04x} - {:04x}]",
                    description.offset_last_rip_table,
                    description.offset_last_rip_table + LAST_RIP_TABLE_BYTE_SIZE - 1
                );
                return Err(uefi::Status::INVALID_PARAMETER.into());
            }

            let interface = unsafe { super::raw::ComInterface::new(description) };
            let size = interface.description.memory_usage();
            let page = PageAllocation::alloc_address(
                PhysicalAddress::from(interface.description.base as u64),
                (size / 4096) + 1,
            )?;

            Ok(Self {
                base: interface,
                allocation: page,
            })
        }

        /// Gets the interface description
        pub const fn description(&self) -> &'a ComInterfaceDescription {
            self.base.description
        }

        /// Reads the entire jump table
        #[track_caller]
        pub fn read_jump_table(&self) -> &[JumpTableEntry] {
            unsafe { self.base.read_jump_table() }
        }

        /// Writes a value to the jump table at the specified index
        #[track_caller]
        pub fn write_jump_table<P: Into<UCInstructionAddress>>(&mut self, index: usize, value: P) {
            unsafe { self.base.write_jump_table(index, value) }
        }

        /// Writes multiple values to the jump table
        #[track_caller]
        pub fn write_jump_table_all<P: Into<UCInstructionAddress>, T: IntoIterator<Item = P>>(
            &mut self,
            values: T,
        ) {
            unsafe { self.base.write_jump_table_all(values) }
        }

        /// Zeros out the entire jump table
        #[track_caller]
        pub fn zero_jump_table(&mut self) {
            unsafe { self.base.zero_jump_table() }
        }

        /// Reads a coverage entry at the specified index
        #[track_caller]
        pub fn read_coverage_table(&self, index: usize) -> CoverageEntry {
            unsafe { self.base.read_coverage_table(index) }
        }

        /// Writes a coverage entry at the specified index
        #[track_caller]
        pub fn write_coverage_table(&mut self, index: usize, value: CoverageEntry) {
            unsafe { self.base.write_coverage_table(index, value) }
        }

        /// Resets all coverage entries to their default values
        #[track_caller]
        pub fn reset_coverage(&mut self) {
            unsafe { self.base.reset_coverage() }
        }

        /// Reads an instruction entry at the specified index
        #[track_caller]
        pub fn read_instruction_table(&self, index: usize) -> InstructionTableEntry {
            self.base.read_instruction_table(index)
        }

        /// Reads the last RIP entry at the specified index
        #[track_caller]
        pub fn read_last_rip(&self, index: usize) -> LastRIPEntry {
            self.base.read_last_rip(index)
        }

        /// Writes an instruction entry at the specified index
        #[track_caller]
        pub fn write_instruction_table(&mut self, index: usize, value: InstructionTableEntry) {
            self.base.write_instruction_table(index, value)
        }

        /// Writes multiple instruction entries
        #[track_caller]
        pub fn write_instruction_table_all<
            P: Into<InstructionTableEntry>,
            T: IntoIterator<Item = P>,
        >(
            &mut self,
            values: T,
        ) {
            unsafe { self.base.write_instruction_table_all(values) }
        }
    }
}

// default os
#[cfg(all(not(feature = "uefi")))]
pub mod safe {
    use crate::interface_definition::{
        ComInterfaceDescription, CoverageEntry, InstructionTableEntry, JumpTableEntry,
    };
    use data_types::addresses::UCInstructionAddress;

    pub struct ComInterface<'a> {
        base: super::raw::ComInterface<'a>,
    }

    impl<'a> crate::interface::safe::ComInterface<'a> {
        pub unsafe fn new(description: &'a ComInterfaceDescription) -> Result<Self, ()> {
            if description.base == 0 {
                return Err(());
            }

            let interface = unsafe { super::raw::ComInterface::new(description) };

            Ok(Self { base: interface })
        }

        pub const fn description(&self) -> &'a ComInterfaceDescription {
            self.base.description
        }

        pub fn read_jump_table(&self) -> &[JumpTableEntry] {
            unsafe { self.base.read_jump_table() }
        }

        pub fn write_jump_table<P: Into<UCInstructionAddress>>(&mut self, index: usize, value: P) {
            unsafe { self.base.write_jump_table(index, value) }
        }

        pub fn write_jump_table_all<P: Into<UCInstructionAddress>, T: IntoIterator<Item = P>>(
            &mut self,
            values: T,
        ) {
            unsafe { self.base.write_jump_table_all(values) }
        }

        pub fn zero_jump_table(&mut self) {
            unsafe { self.base.zero_jump_table() }
        }

        pub fn read_coverage_table(&self, index: usize) -> CoverageEntry {
            unsafe { self.base.read_coverage_table(index) }
        }

        pub fn write_coverage_table(&mut self, index: usize, value: CoverageEntry) {
            unsafe { self.base.write_coverage_table(index, value) }
        }

        pub fn reset_coverage(&mut self) {
            unsafe { self.base.reset_coverage() }
        }

        pub fn read_instruction_table(&self, index: usize) -> InstructionTableEntry {
            self.base.read_instruction_table(index)
        }

        pub fn write_instruction_table(&mut self, index: usize, value: InstructionTableEntry) {
            self.base.write_instruction_table(index, value)
        }

        pub fn write_instruction_table_all<
            P: Into<InstructionTableEntry>,
            T: IntoIterator<Item = P>,
        >(
            &mut self,
            values: T,
        ) {
            unsafe { self.base.write_instruction_table_all(values) }
        }
    }
}
