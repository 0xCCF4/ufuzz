pub const CMOS_IO_INDEX_REGISTER: u8 = 0x70;
pub const CMOS_IO_DATA_REGISTER: u8 = 0x71;
pub const CMOS_IO_EXTENDED_INDEX_REGISTER: u8 = 0;
pub const CMOS_IO_EXTENDED_DATA_REGISTER: u8 = 0;

#[path = "platform_standard.rs"]
mod platform_standard;

pub use platform_standard::*;
