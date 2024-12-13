#[allow(clippy::missing_safety_doc)] // todo: remove this and write safety docs
pub mod raw {
    use crate::interface_definition::{ClockTableEntry, ClockTableSettingsEntry, ComInterfaceDescription, CoverageEntry, InstructionTableEntry, JumpTableEntry};
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
                self.description.max_number_of_hooks.into(),
            )
        }

        pub unsafe fn write_jump_table<P: Into<UCInstructionAddress>>(
            &mut self,
            index: usize,
            value: P,
        ) {
            if index >= self.description.max_number_of_hooks.into() {
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
            for i in 0..self.description.max_number_of_hooks as usize {
                self.write_jump_table(i, UCInstructionAddress::ZERO);
            }
        }

        unsafe fn coverage_table(&self) -> NonNull<CoverageEntry> {
            self.base
                .add(self.description.offset_coverage_result_table)
                .cast()
        }

        pub unsafe fn read_coverage_table(&self, index: usize) -> CoverageEntry {
            if index >= self.description.max_number_of_hooks.into() {
                return 0;
            }

            self.coverage_table().add(index).read_volatile()
        }

        pub unsafe fn write_coverage_table(&mut self, index: usize, value: CoverageEntry) {
            if index >= self.description.max_number_of_hooks.into() {
                return;
            }

            self.coverage_table().add(index).write_volatile(value);
        }

        pub unsafe fn reset_coverage(&mut self) {
            for index in 0..self.description.max_number_of_hooks as usize {
                self.write_coverage_table(index, 0);
            }
        }

        unsafe fn instruction_table(&self) -> NonNull<InstructionTableEntry> {
            self.base
                .add(self.description.offset_instruction_table)
                .cast()
        }

        pub fn read_instruction_table(&self, index: usize) -> InstructionTableEntry {
            if index >= self.description.max_number_of_hooks.into() {
                return [0; 4];
            }

            unsafe { self.instruction_table().add(index).read_volatile() }
        }

        pub fn write_instruction_table(&mut self, index: usize, value: InstructionTableEntry) {
            if index >= self.description.max_number_of_hooks.into() {
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

        unsafe fn clock_table(&self) -> NonNull<ClockTableEntry> {
            self.base
                .add(self.description.offset_clock_table)
                .cast()
        }

        unsafe fn clock_table_settings(&self) -> NonNull<ClockTableSettingsEntry> {
            self.base
                .add(self.description.offset_clock_table_settings)
                .cast()
        }

        pub unsafe fn write_clock_table(&mut self, index: usize, value: ClockTableEntry) {
            if index >= self.description.max_number_of_hooks.into() {
                return;
            }

            self.clock_table().add(index).write_volatile(value);
        }

        pub unsafe fn write_clock_table_settings(&mut self, index: usize, value: ClockTableSettingsEntry) {
            if index >= self.description.max_number_of_hooks.into() {
                return;
            }

            self.clock_table_settings().add(index).write_volatile(value);
        }

        pub unsafe fn write_clock_table_all<T: IntoIterator<Item = ClockTableEntry>>(&mut self, values: T) {
            for (index, value) in values.into_iter().enumerate() {
                self.write_clock_table(index, value);
            }
        }

        pub unsafe fn write_clock_table_settings_all<T: IntoIterator<Item = ClockTableSettingsEntry>>(&mut self, values: T) {
            for (index, value) in values.into_iter().enumerate() {
                self.write_clock_table_settings(index, value);
            }
        }

        pub unsafe fn read_clock_table(&self, index: usize) -> ClockTableEntry {
            if index >= self.description.max_number_of_hooks.into() {
                return 0;
            }

            self.clock_table().add(index).read_volatile()
        }

        pub unsafe fn read_clock_table_settings(&self, index: usize) -> ClockTableSettingsEntry {
            if index >= self.description.max_number_of_hooks.into() {
                return 0;
            }

            self.clock_table_settings().add(index).read_volatile()
        }

        pub unsafe fn zero_clock_table(&mut self) {
            for i in 0..self.description.max_number_of_hooks as usize {
                self.write_clock_table(i, 0);
            }
        }

        pub unsafe fn zero_clock_table_settings(&mut self) {
            for i in 0..self.description.max_number_of_hooks as usize {
                self.write_clock_table_settings(i, 0);
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

        pub fn write_clock_table(&mut self, index: usize, value: u64) {
            unsafe { self.base.write_clock_table(index, value) }
        }

        pub fn write_clock_table_settings(&mut self, index: usize, value: u16) {
            unsafe { self.base.write_clock_table_settings(index, value) }
        }

        pub fn write_clock_table_all<T: IntoIterator<Item = u64>>(&mut self, values: T) {
            unsafe { self.base.write_clock_table_all(values) }
        }

        pub fn write_clock_table_settings_all<T: IntoIterator<Item = u16>>(&mut self, values: T) {
            unsafe { self.base.write_clock_table_settings_all(values) }
        }

        pub fn read_clock_table(&self, index: usize) -> u64 {
            unsafe { self.base.read_clock_table(index) }
        }

        pub fn read_clock_table_settings(&self, index: usize) -> u16 {
            unsafe { self.base.read_clock_table_settings(index) }
        }

        pub fn zero_clock_table(&mut self) {
            unsafe { self.base.zero_clock_table() }
        }

        pub fn zero_clock_table_settings(&mut self) {
            unsafe { self.base.zero_clock_table_settings() }
        }
    }
}

// default os
#[cfg(all(not(feature = "uefi"), not(feature = "no_std")))]
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

            Ok(Self {
                base: interface,
            })
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

        pub fn write_clock_table(&mut self, index: usize, value: u64) {
            unsafe { self.base.write_clock_table(index, value) }
        }

        pub fn write_clock_table_settings(&mut self, index: usize, value: u16) {
            unsafe { self.base.write_clock_table_settings(index, value) }
        }

        pub fn write_clock_table_all<T: IntoIterator<Item = u64>>(&mut self, values: T) {
            unsafe { self.base.write_clock_table_all(values) }
        }

        pub fn write_clock_table_settings_all<T: IntoIterator<Item = u16>>(&mut self, values: T) {
            unsafe { self.base.write_clock_table_settings_all(values) }
        }

        pub fn read_clock_table(&self, index: usize) -> u64 {
            unsafe { self.base.read_clock_table(index) }
        }

        pub fn read_clock_table_settings(&self, index: usize) -> u16 {
            unsafe { self.base.read_clock_table_settings(index) }
        }

        pub fn zero_clock_table(&mut self) {
            unsafe { self.base.zero_clock_table() }
        }

        pub fn zero_clock_table_settings(&mut self) {
            unsafe { self.base.zero_clock_table_settings() }
        }
    }
}
