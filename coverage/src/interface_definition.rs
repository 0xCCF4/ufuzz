// everything is data size 16bit width
pub struct ComInterfaceDescription {
    pub base: usize,
    pub max_number_of_hooks: usize,
    pub offset_jump_back_table: usize,
    pub offset_coverage_result_table: usize,
}

const MAX_NUMBER_OF_HOOKS: usize = 32;

pub const COM_INTERFACE_DESCRIPTION: ComInterfaceDescription = ComInterfaceDescription {
    base: 0x1000,
    max_number_of_hooks: MAX_NUMBER_OF_HOOKS,
    offset_coverage_result_table: 0,
    offset_jump_back_table: MAX_NUMBER_OF_HOOKS,
};
