use core::fmt::{Debug, Formatter};
use core::mem::ManuallyDrop;
use core::ops::{Deref, DerefMut};

#[cfg(feature = "bios_ami")]
mod bios_ami;
#[cfg(feature = "bios_ami")]
pub use bios_ami::*;

#[cfg(feature = "bios_bochs")]
mod bios_bochs;
#[cfg(feature = "bios_bochs")]
pub use bios_bochs::*;

#[cfg(feature = "platform_intel")]
mod platform_intel;
#[cfg(feature = "platform_intel")]
pub use platform_intel::*;

#[cfg(feature = "platform_bochs")]
mod platform_bochs;
use crate::cmos;
#[cfg(feature = "platform_bochs")]
pub use platform_bochs::*;

const CMOS_USER_DATA_SIZE: usize = (CMOS_DATA_SIZE - 4) as usize;

union CMOSData<T> {
    data: core::mem::ManuallyDrop<T>,
    raw: [u8; CMOS_USER_DATA_SIZE],
}

impl<T> CMOSData<T> {
    #[allow(dead_code)]
    const fn size_check() {
        assert!(
            core::mem::size_of::<T>() <= CMOS_USER_DATA_SIZE,
            "Size of T is too large for CMOS"
        );
    }
}

impl<T> Drop for CMOSData<T> {
    fn drop(&mut self) {
        unsafe { ManuallyDrop::drop(&mut self.data) }
    }
}

impl<T: Debug> Debug for CMOSData<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?}", unsafe { &self.data })
    }
}

pub struct CMOS<T> {
    checksum: u32,
    data: CMOSData<T>,
    disable_nmi: bool,
    nmi_guard: Option<NMIGuard>,
}

impl<T> CMOS<T> {
    #[allow(dead_code)]
    pub const fn size_check() {
        CMOSData::<T>::size_check();
    }

    pub fn read_from_ram(disable_nmi: bool) -> Self {
        let mut data = Self {
            data: CMOSData {
                raw: [0; CMOS_USER_DATA_SIZE],
            },
            checksum: 0,
            disable_nmi,
            nmi_guard: Some(NMIGuard::disable_nmi(disable_nmi)),
        };
        data.read_cmos_ram();
        data
    }

    pub fn read_cmos_ram(&mut self) {
        self.checksum = 0;
        self.checksum |=
            unsafe { read_cmos_ram(CMOS_DATA_OFFSET + 0, self.disable_nmi) as u32 } << 0;
        self.checksum |=
            unsafe { read_cmos_ram(CMOS_DATA_OFFSET + 1, self.disable_nmi) as u32 } << 8;
        self.checksum |=
            unsafe { read_cmos_ram(CMOS_DATA_OFFSET + 2, self.disable_nmi) as u32 } << 16;
        self.checksum |=
            unsafe { read_cmos_ram(CMOS_DATA_OFFSET + 3, self.disable_nmi) as u32 } << 24;

        for i in 0..CMOS_USER_DATA_SIZE {
            unsafe {
                self.data.raw[i] = read_cmos_ram(
                    i as u8
                        + CMOS_DATA_OFFSET
                        + (CMOS_DATA_SIZE as usize - CMOS_USER_DATA_SIZE) as u8,
                    self.disable_nmi,
                );
            }
        }
    }

    pub fn write_cmos_ram(&self) {
        unsafe {
            write_cmos_ram(
                CMOS_DATA_OFFSET + 0,
                ((self.checksum >> 0) & 0xFF) as u8,
                self.disable_nmi,
            );
            write_cmos_ram(
                CMOS_DATA_OFFSET + 1,
                ((self.checksum >> 8) & 0xFF) as u8,
                self.disable_nmi,
            );
            write_cmos_ram(
                CMOS_DATA_OFFSET + 2,
                ((self.checksum >> 16) & 0xFF) as u8,
                self.disable_nmi,
            );
            write_cmos_ram(
                CMOS_DATA_OFFSET + 3,
                ((self.checksum >> 24) & 0xFF) as u8,
                self.disable_nmi,
            );

            for i in 0..CMOS_USER_DATA_SIZE {
                write_cmos_ram(
                    i as u8
                        + CMOS_DATA_OFFSET
                        + (CMOS_DATA_SIZE as usize - CMOS_USER_DATA_SIZE) as u8,
                    self.data.raw[i as usize],
                    self.disable_nmi,
                );
            }
        }
    }

    pub fn update_checksum(&mut self) {
        self.checksum = self.calculate_checksum();
    }

    pub fn calculate_checksum(&self) -> u32 {
        let result = unsafe {
            self.data.raw.iter().fold((0u32, 0u8), |(sum, xor), &x| {
                (sum.wrapping_add(x as u32), xor ^ x)
            })
        };
        result.0 ^ ((result.1 as u32) << 24)
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

    pub fn disable_nmi(&mut self, disable_nmi: bool) {
        self.disable_nmi = disable_nmi;
        unsafe {
            cmos::disable_nmi(disable_nmi);
        }
    }

    pub fn close(mut self) -> NMIGuard {
        self.write_cmos_ram();
        self.nmi_guard.take().unwrap()
    }
}

impl<T: Default> CMOS<T> {
    pub fn reset(&mut self) -> CMOSDataHandle<T> {
        self.data = CMOSData {
            data: core::mem::ManuallyDrop::new(T::default()),
        };
        CMOSDataHandle::new(self)
    }
}

impl<T: Default> Default for CMOS<T> {
    fn default() -> Self {
        let nmi = unsafe { is_nmi_disabled() };
        let mut new = Self {
            data: CMOSData {
                data: core::mem::ManuallyDrop::new(T::default()),
            },
            checksum: 0,
            disable_nmi: nmi,
            nmi_guard: Some(NMIGuard::disable_nmi(nmi)),
        };
        new.update_checksum();
        new
    }
}

impl<T: Debug> Debug for CMOS<T> {
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

impl<T> Drop for CMOS<T> {
    fn drop(&mut self) {
        if self.nmi_guard.is_some() {
            self.write_cmos_ram();
            drop(self.nmi_guard.take());
        }
    }
}

pub struct CMOSDataHandle<'a, T> {
    cmos: &'a mut CMOS<T>,
}

impl<'a, T> CMOSDataHandle<'a, T> {
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

pub struct NMIGuard {
    previous_disable_nmi: bool,
}

impl NMIGuard {
    pub fn disable_nmi(disable_nmi: bool) -> Self {
        let previous_disable_nmi = unsafe { is_nmi_disabled() };
        unsafe { cmos::disable_nmi(disable_nmi) };
        Self {
            previous_disable_nmi,
        }
    }
}

impl Drop for NMIGuard {
    fn drop(&mut self) {
        unsafe { cmos::disable_nmi(self.previous_disable_nmi) };
    }
}
