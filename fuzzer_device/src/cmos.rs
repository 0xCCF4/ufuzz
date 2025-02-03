use core::fmt::{Debug, Formatter};
use core::mem::ManuallyDrop;
use core::ops::{Deref, DerefMut};
use log::error;

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

union CMOSData<T>
where
    [(); size_of::<T>()]:,
{
    data: core::mem::ManuallyDrop<T>,
    raw: [u8; size_of::<T>()],
}

impl<T> CMOSData<T>
where
    [(); size_of::<T>()]:,
{
    #[allow(dead_code)]
    const fn size_check() {
        assert!(
            core::mem::size_of::<T>() <= CMOS_USER_DATA_SIZE,
            "Size of T is too large for CMOS"
        );
    }
}

impl<T> Drop for CMOSData<T>
where
    [(); size_of::<T>()]:,
{
    fn drop(&mut self) {
        unsafe { ManuallyDrop::drop(&mut self.data) }
    }
}

impl<T: Debug> Debug for CMOSData<T>
where
    [(); size_of::<T>()]:,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?}", unsafe { &self.data })
    }
}

pub struct CMOS<T>
where
    [(); size_of::<T>()]:,
{
    // todo restructure as enum, such that this is a more safe interface
    data: CMOSData<T>,
    valid: bool,
    disable_nmi: bool,
    nmi_guard: Option<NMIGuard>,
}

impl<T> CMOS<T>
where
    [(); size_of::<T>()]:,
{
    #[allow(dead_code)]
    pub const fn size_check() {
        CMOSData::<T>::size_check();
    }

    #[allow(dead_code)]
    unsafe fn mark_valid(&mut self) {
        self.valid = true;
    }

    pub fn is_data_valid(&self) -> bool {
        self.valid
    }

    pub fn read_from_ram(disable_nmi: bool) -> Self {
        let mut data = Self {
            data: CMOSData {
                raw: [0; size_of::<T>()],
            },
            disable_nmi,
            valid: false,
            nmi_guard: Some(NMIGuard::disable_nmi(disable_nmi)),
        };
        data.read_cmos_ram();
        data
    }

    pub fn read_checksum(&mut self) -> u32 {
        let mut checksum: u32 = 0;
        checksum |= unsafe { read_cmos_ram(CMOS_DATA_OFFSET + 0, self.disable_nmi) as u32 } << 0;
        checksum |= unsafe { read_cmos_ram(CMOS_DATA_OFFSET + 1, self.disable_nmi) as u32 } << 8;
        checksum |= unsafe { read_cmos_ram(CMOS_DATA_OFFSET + 2, self.disable_nmi) as u32 } << 16;
        checksum |= unsafe { read_cmos_ram(CMOS_DATA_OFFSET + 3, self.disable_nmi) as u32 } << 24;
        checksum
    }

    pub fn read_cmos_ram(&mut self) {
        let checksum = self.read_checksum();

        assert!(
            size_of::<T>() <= CMOS_USER_DATA_SIZE,
            "Size of T is too large for CMOS"
        );

        for i in 0..size_of::<T>() {
            unsafe {
                self.data.raw[i] = read_cmos_ram(
                    i as u8 + CMOS_DATA_OFFSET + size_of::<u32>() as u8,
                    self.disable_nmi,
                );
            }
        }

        self.valid = checksum == self.calculate_checksum();
    }

    pub fn write_cmos_ram(&self) {
        if !self.valid {
            error!("Tried to write invalid CMOS data; Skipped writing");
            return;
        }

        assert!(
            size_of::<T>() <= CMOS_USER_DATA_SIZE,
            "Size of T is too large for CMOS"
        );

        let checksum = self.calculate_checksum();

        unsafe {
            write_cmos_ram(
                CMOS_DATA_OFFSET + 0,
                ((checksum >> 0) & 0xFF) as u8,
                self.disable_nmi,
            );
            write_cmos_ram(
                CMOS_DATA_OFFSET + 1,
                ((checksum >> 8) & 0xFF) as u8,
                self.disable_nmi,
            );
            write_cmos_ram(
                CMOS_DATA_OFFSET + 2,
                ((checksum >> 16) & 0xFF) as u8,
                self.disable_nmi,
            );
            write_cmos_ram(
                CMOS_DATA_OFFSET + 3,
                ((checksum >> 24) & 0xFF) as u8,
                self.disable_nmi,
            );

            for i in 0..size_of::<T>() {
                write_cmos_ram(
                    i as u8 + CMOS_DATA_OFFSET + size_of::<u32>() as u8,
                    self.data.raw[i as usize],
                    self.disable_nmi,
                );
            }
        }
    }

    fn calculate_checksum(&self) -> u32 {
        let result = unsafe {
            self.data.raw.iter().fold((0u32, 0u8), |(sum, xor), &x| {
                (sum.wrapping_add(x as u32), xor ^ x)
            })
        };
        result.0 ^ ((result.1 as u32) << 24)
    }

    pub fn data(&self) -> Option<&T> {
        if self.valid {
            Some(unsafe { &self.data.data })
        } else {
            None
        }
    }

    pub fn data_mut(&mut self) -> Option<CMOSDataHandle<T>> {
        if self.valid {
            Some(CMOSDataHandle::new(self))
        } else {
            None
        }
    }

    pub unsafe fn raw_data(&mut self) -> &mut [u8; size_of::<T>()] {
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

impl<T: Default> CMOS<T>
where
    [(); size_of::<T>()]:,
{
    pub fn reset(&mut self) -> CMOSDataHandle<T> {
        self.data = CMOSData {
            data: core::mem::ManuallyDrop::new(T::default()),
        };
        self.valid = true;
        CMOSDataHandle::new(self)
    }

    pub fn data_mut_or_insert(&mut self) -> CMOSDataHandle<T> {
        if !self.valid {
            self.reset()
        } else {
            CMOSDataHandle::new(self)
        }
    }

    pub fn data_or_insert(&mut self) -> &T {
        if !self.valid {
            self.reset();
        }
        unsafe { &self.data.data }
    }
}

impl<T: Default> Default for CMOS<T>
where
    [(); size_of::<T>()]:,
{
    fn default() -> Self {
        let nmi = unsafe { is_nmi_disabled() };
        let new = Self {
            data: CMOSData {
                data: core::mem::ManuallyDrop::new(T::default()),
            },
            valid: true,
            disable_nmi: nmi,
            nmi_guard: Some(NMIGuard::disable_nmi(nmi)),
        };
        new
    }
}

impl<T: Debug> Debug for CMOS<T>
where
    [(); size_of::<T>()]:,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        unsafe {
            if self.valid {
                f.debug_struct("CMOS@Valid")
                    .field("data", &self.data.data)
                    .finish()
            } else {
                f.debug_struct("CMOS@Invalid")
                    .field("data", &self.data.raw)
                    .finish()
            }
        }
    }
}

impl<T> Drop for CMOS<T>
where
    [(); size_of::<T>()]:,
{
    fn drop(&mut self) {
        if self.nmi_guard.is_some() {
            self.write_cmos_ram();
            drop(self.nmi_guard.take());
        }
    }
}

pub struct CMOSDataHandle<'a, T>
where
    [(); size_of::<T>()]:,
{
    cmos: &'a mut CMOS<T>,
}

impl<'a, T> CMOSDataHandle<'a, T>
where
    [(); size_of::<T>()]:,
{
    fn new(cmos: &'a mut CMOS<T>) -> Self {
        Self { cmos }
    }
}

impl<'a, T> DerefMut for CMOSDataHandle<'a, T>
where
    [(); size_of::<T>()]:,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut self.cmos.data.data }
    }
}

impl<'a, T> Deref for CMOSDataHandle<'a, T>
where
    [(); size_of::<T>()]:,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &self.cmos.data.data }
    }
}

impl<'a, T> Drop for CMOSDataHandle<'a, T>
where
    [(); size_of::<T>()]:,
{
    fn drop(&mut self) {
        self.cmos.write_cmos_ram();
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
