use core::arch::asm;
use core::fmt::{Debug, Formatter};
use core::mem::ManuallyDrop;
use core::ops::{Deref, DerefMut};

#[cfg(any(feature = "platform_intel_celeron_n_ami", feature = "platform_other"))]
const CMOS_IO_INDEX_REGISTER: u8 = 0x70;
#[cfg(any(feature = "platform_intel_celeron_n_ami", feature = "platform_other"))]
const CMOS_IO_DATA_REGISTER: u8 = 0x71;
#[cfg(feature = "platform_intel_celeron_n_ami")]
const CMOS_IO_EXTENDED_INDEX_REGISTER: u8 = 0x72;
#[cfg(feature = "platform_intel_celeron_n_ami")]
const CMOS_IO_EXTENDED_DATA_REGISTER: u8 = 0x73;
#[cfg(feature = "platform_intel_celeron_n_ami")]
const CMOS_DATA_OFFSET: u8 = 0x80;
#[cfg(feature = "platform_intel_celeron_n_ami")]
pub const CMOS_DATA_SIZE: u8 = 64;

#[cfg(feature = "platform_other")]
const CMOS_DATA_OFFSET: u8 = 0x0;

#[cfg(feature = "platform_other")]
pub const CMOS_DATA_SIZE: u8 = 128;


#[cfg(any(feature = "platform_intel_celeron_n_ami"))]
#[inline]
unsafe fn write_cmos_ram(index: u8, value: u8, nmi_disable: bool) {
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

#[cfg(not(all(feature = "platform_intel_celeron_n_ami")))]
#[inline]
unsafe fn write_cmos_ram(index: u8, value: u8, nmi_disable: bool) {
    write_cmos_index(index, nmi_disable);
}

#[cfg(any(feature = "platform_intel_celeron_n_ami"))]
#[inline]
unsafe fn read_cmos_ram(index: u8, nmi_disable: bool) -> u8 {
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

pub unsafe fn disable_nmi(nmi_disable: bool) {
    let nmi_disable: u8 = if nmi_disable {
        0x80
    } else {
        0x00
    };
    asm!(
    "out dx, al",
    in("dx") CMOS_IO_INDEX_REGISTER as u16,
    in("al") nmi_disable,
    )
}

#[cfg(not(all(feature = "platform_intel_celeron_n_ami")))]
unsafe fn read_cmos_ram(index: u8, nmi_disable: bool) -> u8 {
    write_cmos_index(index, nmi_disable);
    0
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

const CMOS_USER_DATA_SIZE: usize = (CMOS_DATA_SIZE - 4) as usize;

union CMOSData<T> {
    data: core::mem::ManuallyDrop<T>,
    raw: [u8; CMOS_USER_DATA_SIZE],
}

impl<T> CMOSData<T> {
    #[allow(dead_code)]
    const fn size_check() {
        assert!(core::mem::size_of::<T>() <= CMOS_USER_DATA_SIZE, "Size of T is too large for CMOS");
    }
}

impl<T> Drop for CMOSData<T>  {
    fn drop(&mut self) {
        unsafe { ManuallyDrop::drop(&mut self.data) }
    }
}

impl<T: Debug> Debug for CMOSData<T>  {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?}", unsafe { &self.data })
    }
}

pub struct CMOS<T>  {
    checksum: u32,
    data: CMOSData<T>,
}

impl<T> CMOS<T>  {
    #[allow(dead_code)]
    pub const fn size_check() {
        CMOSData::<T>::size_check();
    }

    pub fn new(disable_nmi: bool) -> Self {
        let mut data = Self {
            data: CMOSData {
                raw: [0; CMOS_USER_DATA_SIZE],
            },
            checksum: 0,
        };
        data.read_cmos_ram(disable_nmi);
        data
    }

    pub fn read_cmos_ram(&mut self, disable_nmi: bool) {
        self.checksum = 0;
        self.checksum |= unsafe { read_cmos_ram(CMOS_DATA_OFFSET+0, disable_nmi) as u32 } << 0;
        self.checksum |= unsafe { read_cmos_ram(CMOS_DATA_OFFSET+1, disable_nmi) as u32 } << 8;
        self.checksum |= unsafe { read_cmos_ram(CMOS_DATA_OFFSET+2, disable_nmi) as u32 } << 16;
        self.checksum |= unsafe { read_cmos_ram(CMOS_DATA_OFFSET+3, disable_nmi) as u32 } << 24;

        for i in 0..CMOS_USER_DATA_SIZE {
            unsafe {
                self.data.raw[i] = read_cmos_ram(i as u8 + CMOS_DATA_OFFSET + (CMOS_DATA_SIZE as usize-CMOS_USER_DATA_SIZE) as u8, disable_nmi);
            }
        }
    }

    pub fn write_cmos_ram(&self, disable_nmi: bool) {
        unsafe {
            write_cmos_ram(CMOS_DATA_OFFSET+0, ((self.checksum >> 0) & 0xFF) as u8, disable_nmi);
            write_cmos_ram(CMOS_DATA_OFFSET+1, ((self.checksum >> 8) & 0xFF) as u8, disable_nmi);
            write_cmos_ram(CMOS_DATA_OFFSET+2, ((self.checksum >> 16) & 0xFF) as u8, disable_nmi);
            write_cmos_ram(CMOS_DATA_OFFSET+3, ((self.checksum >> 24) & 0xFF) as u8, disable_nmi);

            for i in 0..CMOS_USER_DATA_SIZE {
                write_cmos_ram(i as u8 + CMOS_DATA_OFFSET + (CMOS_DATA_SIZE as usize-CMOS_USER_DATA_SIZE) as u8, self.data.raw[i as usize], disable_nmi);
            }
        }
    }

    pub fn update_checksum(&mut self) {
        self.checksum = self.calculate_checksum();
    }

    pub fn calculate_checksum(&self) -> u32 {
        unsafe {
            self.data.raw.iter().fold(0, |acc, &x| acc.wrapping_add(x as u32))
        }
    }

    pub fn checksum_valid(&self) -> bool {
        self.checksum == self.calculate_checksum()
    }

    pub fn data(&self) -> Option<&T> {
        if self.checksum_valid() {
            Some(unsafe { &self.data.data })
        } else {
            None
        }
    }
    
    pub fn data_mut<'a>(&'a mut self) -> Option<CMOSDataHandle<'a, T>> {
        if self.checksum_valid() {
            Some(CMOSDataHandle::new(self))
        } else {
            None
        }
    }

    pub unsafe fn raw_data(&mut self) -> &mut [u8; CMOS_USER_DATA_SIZE] {
        &mut self.data.raw
    }
}

impl<T: Default> CMOS<T> 
{
    pub fn reset(&mut self) -> CMOSDataHandle<T> {
        self.data = CMOSData {
            data: core::mem::ManuallyDrop::new(T::default())
        };
        CMOSDataHandle::new(self)
    }
}

impl<T: Default> Default for CMOS<T>  {
    fn default() -> Self {
        let mut new = Self {
            data: CMOSData {
                data: core::mem::ManuallyDrop::new(T::default())
            },
            checksum: 0,
        };
        new.update_checksum();
        new
    }
}

impl<T: Debug> Debug for CMOS<T> 
{
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        unsafe {
            f.debug_struct("CMOS")
                .field("checksum", &self.checksum)
                .field("checksum_valid", &self.checksum_valid())
                .field("data", &self.data.data)
                .finish()
        }
    }
}

pub struct CMOSDataHandle<'a, T>  {
    cmos: &'a mut CMOS<T>,
}

impl<'a, T> CMOSDataHandle<'a, T>  {
    fn new(cmos: &'a mut CMOS<T>) -> Self {
        Self { cmos }
    }
}

impl<'a, T> DerefMut for CMOSDataHandle<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut self.cmos.data.data }
    }
}

impl<'a, T> Deref for CMOSDataHandle<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &self.cmos.data.data }
    }
}

impl<'a, T> Drop for CMOSDataHandle<'a, T> {
    fn drop(&mut self) {
        self.cmos.update_checksum();
    }
}