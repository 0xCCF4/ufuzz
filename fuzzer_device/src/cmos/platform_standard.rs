use crate::cmos::{
    CMOS_IO_DATA_REGISTER, CMOS_IO_EXTENDED_DATA_REGISTER, CMOS_IO_EXTENDED_INDEX_REGISTER,
    CMOS_IO_INDEX_REGISTER,
};
use core::arch::asm;

#[inline]
pub unsafe fn write_cmos_ram(index: u8, value: u8, nmi_disable: bool) {
    let truncated_index = index & 0x7f;
    let nmi_disable = if nmi_disable && index < 128 {
        0x80
    } else {
        0x00
    };
    let truncated_index = nmi_disable | (truncated_index & 0x7f);
    let index_port = if index < 128 {
        // standard bank
        CMOS_IO_INDEX_REGISTER
    } else {
        // extended bank
        CMOS_IO_EXTENDED_INDEX_REGISTER
    };
    let data_port = if index < 128 {
        CMOS_IO_DATA_REGISTER
    } else {
        CMOS_IO_EXTENDED_DATA_REGISTER
    };

    asm!(
    "pushf",
    "cli",
    "mov dx, {index_port:x}",
    "mov al, {truncated_index}",
    "out dx, al",
    "mov dx, {data_port:x}",
    "mov al, {value}",
    "out dx, al",
    "popf",
    index_port = in(reg) index_port as u16,
    data_port = in(reg) data_port as u16,
    truncated_index = in(reg_byte) truncated_index,
    value = in(reg_byte) value,
    out("dx") _,
    out("al") _,
    );
}

#[inline]
pub unsafe fn read_cmos_ram(index: u8, nmi_disable: bool) -> u8 {
    let truncated_index = index & 0x7f;
    let nmi_disable = if nmi_disable && index < 128 {
        0x80
    } else {
        0x00
    };
    let truncated_index = nmi_disable | (truncated_index & 0x7f);
    let index_port = if index < 128 {
        // standard bank
        CMOS_IO_INDEX_REGISTER
    } else {
        // extended bank
        CMOS_IO_EXTENDED_INDEX_REGISTER
    };
    let data_port = if index < 128 {
        CMOS_IO_DATA_REGISTER
    } else {
        CMOS_IO_EXTENDED_DATA_REGISTER
    };

    let value: u8;

    asm!(
    "pushf",
    "cli",
    "mov dx, {index_port:x}",
    "mov al, {truncated_index}",
    "out dx, al",
    "mov dx, {data_port:x}",
    "in al, dx",
    "popf",
    index_port = in(reg) index_port as u16,
    data_port = in(reg) data_port as u16,
    truncated_index = in(reg_byte) truncated_index,
    out("al") value,
    out("dx") _,
    );

    value
}

#[inline]
pub unsafe fn disable_nmi(nmi_disable: bool) {
    let nmi_disable: u8 = if nmi_disable { 0x80 } else { 0x00 };
    asm!(
    "out dx, al",
    in("dx") CMOS_IO_INDEX_REGISTER as u16,
    in("al") nmi_disable,
    )
}

#[inline]
#[allow(dead_code)]
pub unsafe fn is_nmi_disabled() -> bool {
    let value: u8;
    asm!(
    "in al, {index_port}",
    out("al") value,
    index_port = const CMOS_IO_INDEX_REGISTER,
    );
    value & 0x80 != 0
}
