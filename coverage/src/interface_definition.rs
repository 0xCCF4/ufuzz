// everything is data size 16bit width
pub struct ComInterfaceDescription {
    pub base: usize,
    pub max_number_of_hooks: usize,
    pub offset_coverage_result_table: usize,
    // pub offset_timing_table: usize,
    pub offset_jump_back_table: usize,
}

impl ComInterfaceDescription {
    #[allow(dead_code)] // todo: maybe cargo error, since it is actually used, investigate and if valid, report to cargo
    pub const fn memory_usage(&self) -> usize {
        let jump = self.offset_jump_back_table + self.max_number_of_hooks * 2;
        let coverage = self.offset_coverage_result_table + self.max_number_of_hooks * 2;

        if jump > coverage {
            jump
        } else {
            coverage
        }
    }
}

const MAX_NUMBER_OF_HOOKS: usize = 20;

pub const COM_INTERFACE_DESCRIPTION: ComInterfaceDescription = ComInterfaceDescription {
    base: 0x1000,
    max_number_of_hooks: MAX_NUMBER_OF_HOOKS,
    offset_coverage_result_table: 0,
    offset_jump_back_table: MAX_NUMBER_OF_HOOKS,
};
