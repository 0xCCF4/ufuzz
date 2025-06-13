//! CMOS Memory Management Module
//!
//! This module provides functionality for reading and writing data to CMOS memory,
//! which persists across system reboots.

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

/// Maximum size of user data that can be stored in CMOS memory
const CMOS_USER_DATA_SIZE: usize = (CMOS_DATA_SIZE - 4) as usize;

/// Internal union type for storing data in CMOS memory
///
/// This union allows for both type-safe access to the data and raw byte access
/// for checksum calculation and storage.
union CMOSData<T>
where
    [(); size_of::<T>()]:,
{
    /// Type-safe access to the stored data
    data: core::mem::ManuallyDrop<T>,
    /// Raw byte access for storage and checksum calculation
    raw: [u8; size_of::<T>()],
}

impl<T> CMOSData<T>
where
    [(); size_of::<T>()]:,
{
    /// Verifies that the type T fits within the available CMOS memory
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

/// Main structure for managing CMOS memory access
///
/// This structure provides a safe interface for reading and writing data to CMOS memory,
/// including checksum validation and NMI management.
///
/// # Type Parameters
///
/// * `T` - The type of data to store in CMOS memory
///
/// # Safety
///
/// The type `T` must be `repr(C)` to ensure proper memory layout when stored in CMOS.
pub struct CMOS<T>
where
    [(); size_of::<T>()]:,
{
    /// The data stored in CMOS memory
    data: CMOSData<T>,
    /// Whether the stored data is valid (checksum matches)
    valid: bool,
    /// Whether NMI is disabled during CMOS operations
    disable_nmi: bool,
    /// Guard for managing NMI state
    nmi_guard: Option<NMIGuard>,
}

impl<T> CMOS<T>
where
    [(); size_of::<T>()]:,
{
    /// Verifies that the type T fits within the available CMOS memory
    #[allow(dead_code)]
    pub const fn size_check() {
        CMOSData::<T>::size_check();
    }

    /// Marks the stored data as valid
    ///
    /// # Safety
    ///
    /// This function should only be called when the data is known to be valid.
    #[allow(dead_code)]
    unsafe fn mark_valid(&mut self) {
        self.valid = true;
    }

    /// Checks if the stored data is valid
    pub fn is_data_valid(&self) -> bool {
        self.valid
    }

    /// Reads data from CMOS memory into a new instance
    ///
    /// # Arguments
    ///
    /// * `disable_nmi` - Whether to disable NMI during the use of the returned data
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

    /// Reads the checksum from CMOS memory
    pub fn read_checksum(&mut self) -> u32 {
        let mut checksum: u32 = 0;
        checksum |= unsafe { read_cmos_ram(CMOS_DATA_OFFSET + 0, self.disable_nmi) as u32 } << 0;
        checksum |= unsafe { read_cmos_ram(CMOS_DATA_OFFSET + 1, self.disable_nmi) as u32 } << 8;
        checksum |= unsafe { read_cmos_ram(CMOS_DATA_OFFSET + 2, self.disable_nmi) as u32 } << 16;
        checksum |= unsafe { read_cmos_ram(CMOS_DATA_OFFSET + 3, self.disable_nmi) as u32 } << 24;
        checksum
    }

    /// Reads data from CMOS memory and validates it
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

    /// Writes data to CMOS memory if it is valid
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

    /// Calculates the checksum of the stored data
    fn calculate_checksum(&self) -> u32 {
        let result = unsafe {
            self.data.raw.iter().fold((0u32, 0u8), |(sum, xor), &x| {
                (sum.wrapping_add(x as u32), xor ^ x)
            })
        };
        result.0 ^ ((result.1 as u32) << 24)
    }

    /// Returns a reference to the stored data if it is valid
    pub fn data(&self) -> Option<&T> {
        if self.valid {
            Some(unsafe { &self.data.data })
        } else {
            None
        }
    }

    /// Returns a mutable handle to the stored data if it is valid
    pub fn data_mut(&mut self) -> Option<CMOSDataHandle<T>> {
        if self.valid {
            Some(CMOSDataHandle::new(self))
        } else {
            None
        }
    }

    /// Returns a mutable reference to the raw data
    ///
    /// # Safety
    ///
    /// This function provides direct access to the raw data without validation.
    pub unsafe fn raw_data(&mut self) -> &mut [u8; size_of::<T>()] {
        &mut self.data.raw
    }

    /// Sets whether NMI should be disabled during operations
    pub fn disable_nmi(&mut self, disable_nmi: bool) {
        self.disable_nmi = disable_nmi;
        unsafe {
            cmos::disable_nmi(disable_nmi);
        }
    }

    /// Closes the CMOS instance and returns the NMI guard
    pub fn close(mut self) -> NMIGuard {
        self.write_cmos_ram();
        self.nmi_guard.take().unwrap()
    }
}

impl<T: Default> CMOS<T>
where
    [(); size_of::<T>()]:,
{
    /// Resets the stored data to its default value
    pub fn reset(&mut self) -> CMOSDataHandle<T> {
        self.data = CMOSData {
            data: core::mem::ManuallyDrop::new(T::default()),
        };
        self.valid = true;
        CMOSDataHandle::new(self)
    }

    /// Returns a mutable handle to the data, creating default data if invalid
    pub fn data_mut_or_insert(&mut self) -> CMOSDataHandle<T> {
        if !self.valid {
            self.reset()
        } else {
            CMOSDataHandle::new(self)
        }
    }

    /// Returns a reference to the data, creating default data if invalid
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

/// Handle for safely modifying CMOS data
///
/// This structure provides a safe way to modify CMOS data while ensuring
/// proper cleanup and validation.
pub struct CMOSDataHandle<'a, T>
where
    [(); size_of::<T>()]:,
{
    /// Reference to the parent CMOS instance
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

/// Guard for managing NMI state
///
/// This structure ensures that NMI state is properly restored when dropped.
pub struct NMIGuard {
    /// The previous NMI state
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
