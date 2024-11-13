use core::ptr::NonNull;
use crate::interface_definition::ComInterfaceDescription;

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
        self.base.offset(self.description.offset_jump_back_table as isize)
    }

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
        self.jump_table().offset(index as isize).write_volatile(value);
    }

    pub unsafe fn write_jump_table_all(&mut self, values: &[u16]) {
        for (index, value) in values.iter().enumerate() {
            self.write_jump_table(index, *value);
        }
    }

    unsafe fn coverage_table(&self) -> NonNull<u16> {
        self.base.offset(self.description.offset_coverage_result_table as isize)
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

        self.coverage_table().offset(index as isize).write_volatile(value);
    }

    pub unsafe fn reset_coverage(&mut self) {
        for index in 0..self.description.max_number_of_hooks {
            self.write_coverage_table(index, 0);
        }
    }

    /*
    unsafe fn time_table(&self) -> NonNull<u16> {
        self.base.offset(self.description.offset_timing_table as isize)
    }

    pub unsafe fn read_time_table(&self, index: usize) -> u16 {
        if index >= self.description.max_number_of_hooks {
            return 0;
        }

        self.time_table().offset(index as isize).read_volatile()
    }

    pub unsafe fn write_time_table(&mut self, index: usize, value: u16) {
        if index >= self.description.max_number_of_hooks {
            return;
        }

        self.time_table().offset(index as isize).write_volatile(value);
    }

    pub unsafe fn reset_time_table(&mut self) {
        for index in 0..self.description.max_number_of_hooks {
            self.write_time_table(index, 0);
        }
    }
    */
}
