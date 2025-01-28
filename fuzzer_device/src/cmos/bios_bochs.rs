pub const CMOS_DATA_OFFSET: u8 = 0x80;
#[cfg(feature = "platform_bochs")]
pub const CMOS_DATA_SIZE: u8 = 128;
#[cfg(not(feature = "platform_bochs"))]
pub const CMOS_DATA_SIZE: u8 = 0;
