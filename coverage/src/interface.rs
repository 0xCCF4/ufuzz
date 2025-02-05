#[allow(clippy::missing_safety_doc)] // todo: remove this and write safety docs
pub mod raw {
    use crate::interface_definition::{
        ComInterfaceDescription, CoverageEntry, InstructionTableEntry, JumpTableEntry,
    };
    use core::ptr::NonNull;
    use data_types::addresses::{Address, UCInstructionAddress};

    pub struct ComInterface<'a> {
        base: NonNull<u8>,
        pub description: &'a ComInterfaceDescription,
    }

    impl<'a> ComInterface<'a> {
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

        unsafe fn jump_table(&self) -> NonNull<JumpTableEntry> {
            self.base
                .add(self.description.offset_jump_back_table)
                .cast()
        }

        #[allow(unused)]
        pub unsafe fn read_jump_table(&self) -> &[JumpTableEntry] {
            core::slice::from_raw_parts(
                self.jump_table().as_ptr(),
                self.description.max_number_of_hooks,
            )
        }

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

        pub unsafe fn zero_jump_table(&mut self) {
            for i in 0..self.description.max_number_of_hooks {
                self.write_jump_table(i, UCInstructionAddress::ZERO);
            }
        }

        unsafe fn coverage_table(&self) -> NonNull<CoverageEntry> {
            self.base
                .add(self.description.offset_coverage_result_table)
                .cast()
        }

        pub unsafe fn read_coverage_table(&self, index: usize) -> CoverageEntry {
            if index >= self.description.max_number_of_hooks {
                return CoverageEntry::default();
            }

            self.coverage_table().add(index).read_volatile()
        }

        pub unsafe fn write_coverage_table(&mut self, index: usize, value: CoverageEntry) {
            if index >= self.description.max_number_of_hooks {
                return;
            }

            self.coverage_table().add(index).write_volatile(value);
        }

        pub unsafe fn reset_coverage(&mut self) {
            for index in 0..self.description.max_number_of_hooks * 2 {
                self.write_coverage_table(index, CoverageEntry::default());
            }
        }

        unsafe fn instruction_table(&self) -> NonNull<InstructionTableEntry> {
            self.base
                .add(self.description.offset_instruction_table)
                .cast()
        }

        pub fn read_instruction_table(&self, index: usize) -> InstructionTableEntry {
            if index >= self.description.max_number_of_hooks {
                return InstructionTableEntry::default();
            }

            unsafe { self.instruction_table().add(index).read_volatile() }
        }

        pub fn write_instruction_table(&mut self, index: usize, value: InstructionTableEntry) {
            if index >= self.description.max_number_of_hooks {
                return;
            }

            unsafe { self.instruction_table().add(index).write_volatile(value) }
        }

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
    }
}

#[cfg(feature = "uefi")]
pub mod safe {
    use crate::interface_definition::{
        ComInterfaceDescription, CoverageEntry, InstructionTableEntry, JumpTableEntry,
    };
    use crate::page_allocation::PageAllocation;
    use data_types::addresses::UCInstructionAddress;
    use uefi::data_types::PhysicalAddress;

    pub struct ComInterface<'a> {
        base: super::raw::ComInterface<'a>,
        #[allow(dead_code)]
        // this is required since, while PageAllocation is alive, the memory is reserved
        allocation: PageAllocation,
    }

    impl<'a> ComInterface<'a> {
        pub fn new(description: &'a ComInterfaceDescription) -> uefi::Result<Self> {
            if description.base == 0 {
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

        pub const fn description(&self) -> &'a ComInterfaceDescription {
            self.base.description
        }

        #[track_caller]
        pub fn read_jump_table(&self) -> &[JumpTableEntry] {
            unsafe { self.base.read_jump_table() }
        }

        #[track_caller]
        pub fn write_jump_table<P: Into<UCInstructionAddress>>(&mut self, index: usize, value: P) {
            unsafe { self.base.write_jump_table(index, value) }
        }

        #[track_caller]
        pub fn write_jump_table_all<P: Into<UCInstructionAddress>, T: IntoIterator<Item = P>>(
            &mut self,
            values: T,
        ) {
            unsafe { self.base.write_jump_table_all(values) }
        }

        #[track_caller]
        pub fn zero_jump_table(&mut self) {
            unsafe { self.base.zero_jump_table() }
        }

        #[track_caller]
        pub fn read_coverage_table(&self, index: usize) -> CoverageEntry {
            unsafe { self.base.read_coverage_table(index) }
        }

        #[track_caller]
        pub fn write_coverage_table(&mut self, index: usize, value: CoverageEntry) {
            unsafe { self.base.write_coverage_table(index, value) }
        }

        #[track_caller]
        pub fn reset_coverage(&mut self) {
            unsafe { self.base.reset_coverage() }
        }

        #[track_caller]
        pub fn read_instruction_table(&self, index: usize) -> InstructionTableEntry {
            self.base.read_instruction_table(index)
        }

        #[track_caller]
        pub fn write_instruction_table(&mut self, index: usize, value: InstructionTableEntry) {
            self.base.write_instruction_table(index, value)
        }

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
#[cfg(all(not(feature = "uefi"), not(feature = "nostd")))]
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
