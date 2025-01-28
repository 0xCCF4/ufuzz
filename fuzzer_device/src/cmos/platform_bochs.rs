pub const CMOS_IO_INDEX_REGISTER: u8 = 0x70;
pub const CMOS_IO_DATA_REGISTER: u8 = 0;
pub const CMOS_IO_EXTENDED_INDEX_REGISTER: u8 = 0;
pub const CMOS_IO_EXTENDED_DATA_REGISTER: u8 = 0;

#[path = "platform_standard.rs"]
mod platform_standard;
pub use platform_standard::{disable_nmi, is_nmi_disabled};

#[inline]
pub unsafe fn read_cmos_ram(index: u8, nmi_disable: bool) -> u8 {
    disable_nmi(nmi_disable);
    0xFF
}

#[inline]
pub unsafe fn write_cmos_ram(index: u8, value: u8, nmi_disable: bool) {
    disable_nmi(nmi_disable);
}
