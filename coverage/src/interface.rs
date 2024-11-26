#[allow(clippy::missing_safety_doc)] // todo: remove this and write safety docs
pub mod raw {
    use crate::interface_definition::ComInterfaceDescription;
    use core::ptr::NonNull;
    use data_types::addresses::{Address, UCInstructionAddress};

    pub struct ComInterface<'a> {
        base: NonNull<u16>,
        pub description: &'a ComInterfaceDescription,
    }

    impl<'a> ComInterface<'a> {
        pub unsafe fn new(description: &'a ComInterfaceDescription) -> Self {
            if description.base == 0 {
                panic!("Invalid base address");
            }

            let ptr = NonNull::new_unchecked(description.base as *mut u16);

            Self {
                base: ptr,
                description,
            }
        }

        unsafe fn jump_table(&self) -> NonNull<u16> {
            self.base
                .offset(self.description.offset_jump_back_table as isize)
        }

        #[allow(unused)]
        pub unsafe fn read_jump_table(&self) -> &[u16] {
            core::slice::from_raw_parts(
                self.jump_table().as_ptr(),
                self.description.max_number_of_hooks,
            )
        }

        pub unsafe fn write_jump_table(&mut self, index: usize, value: u16) {
            if index >= self.description.max_number_of_hooks {
                return;
            }
            self.jump_table()
                .offset(index as isize)
                .write_volatile(value);
        }

        pub unsafe fn write_jump_table_all<
            P: Into<UCInstructionAddress>,
            T: IntoIterator<Item = P>,
        >(
            &mut self,
            values: T,
        ) {
            for (index, value) in values.into_iter().enumerate() {
                self.write_jump_table(index, value.into().address() as u16);
            }
        }

        pub unsafe fn zero_jump_table(&mut self) {
            for i in 0..self.description.max_number_of_hooks {
                self.write_jump_table(i, 0);
            }
        }

        unsafe fn coverage_table(&self) -> NonNull<u16> {
            self.base
                .offset(self.description.offset_coverage_result_table as isize)
        }

        pub unsafe fn read_coverage_table(&self, index: usize) -> u16 {
            if index >= self.description.max_number_of_hooks {
                return 0;
            }

            self.coverage_table().offset(index as isize).read_volatile()
        }

        pub unsafe fn write_coverage_table(&mut self, index: usize, value: u16) {
            if index >= self.description.max_number_of_hooks {
                return;
            }

            self.coverage_table()
                .offset(index as isize)
                .write_volatile(value);
        }

        pub unsafe fn reset_coverage(&mut self) {
            for index in 0..self.description.max_number_of_hooks {
                self.write_coverage_table(index, 0);
            }
        }
    }
}

#[cfg(feature = "uefi")]
pub mod safe {
    use crate::interface_definition::ComInterfaceDescription;
    use crate::page_allocation::PageAllocation;
    use data_types::addresses::UCInstructionAddress;
    use uefi::data_types::PhysicalAddress;

    pub struct ComInterface<'a> {
        base: super::raw::ComInterface<'a>,
        #[allow(dead_code)] // this is required since, while PageAllocation is alive, the memory is reserved
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

        pub fn read_jump_table(&self) -> &[u16] {
            unsafe { self.base.read_jump_table() }
        }

        pub fn write_jump_table(&mut self, index: usize, value: u16) {
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

        pub fn read_coverage_table(&self, index: usize) -> u16 {
            unsafe { self.base.read_coverage_table(index) }
        }

        pub fn write_coverage_table(&mut self, index: usize, value: u16) {
            unsafe { self.base.write_coverage_table(index, value) }
        }

        pub fn reset_coverage(&mut self) {
            unsafe { self.base.reset_coverage() }
        }
    }
}
