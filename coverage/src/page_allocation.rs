//! This module provides UEFI-specific page allocation functionality.
//! It manages memory allocation and deallocation in a UEFI environment, ensuring proper
//! cleanup when allocations are dropped.

use core::mem;
use core::ptr::NonNull;
use uefi::boot::{AllocateType, MemoryType};
use uefi::data_types::PhysicalAddress;

/// Represents an allocated block of memory pages in UEFI
///
/// This structure manages the lifecycle of allocated pages, automatically
/// freeing them when dropped.
pub struct PageAllocation {
    /// Base pointer to the allocated memory
    base: NonNull<u8>,
    /// Number of pages allocated
    count: usize,
}

impl Drop for PageAllocation {
    fn drop(&mut self) {
        unsafe { uefi::boot::free_pages(self.base, self.count).expect("Unable to free pages") }
    }
}

impl PageAllocation {
    /// Allocates pages at a specific physical address
    ///
    /// # Arguments
    ///
    /// * `address` - Physical address where pages should be allocated
    /// * `count` - Number of pages to allocate
    ///
    /// # Returns
    ///
    /// A Result containing the page allocation or a UEFI error
    pub fn alloc_address(address: PhysicalAddress, count: usize) -> uefi::Result<PageAllocation> {
        let data = uefi::boot::allocate_pages(
            AllocateType::Address(address),
            MemoryType::LOADER_DATA,
            count,
        )?;
        Ok(PageAllocation { count, base: data })
    }

    /// Allocates pages at any available address
    ///
    /// # Arguments
    ///
    /// * `count` - Number of pages to allocate
    ///
    /// # Returns
    ///
    /// A Result containing the page allocation or a UEFI error
    pub fn alloc(count: usize) -> uefi::Result<PageAllocation> {
        let data =
            uefi::boot::allocate_pages(AllocateType::AnyPages, MemoryType::LOADER_DATA, count)?;
        Ok(PageAllocation { count, base: data })
    }

    /// Gets a reference to the base pointer
    #[allow(unused)]
    pub fn ptr(&self) -> &NonNull<u8> {
        &self.base
    }

    /// Explicitly deallocates the pages
    pub fn dealloc(self) {
        drop(self)
    }

    /// Gets the physical address of the allocation
    pub fn address(&self) -> usize {
        unsafe { mem::transmute(self.base.cast::<()>()) }
    }
}

/// Implements AsRef for getting a reference to the base pointer
impl AsRef<NonNull<u8>> for PageAllocation {
    fn as_ref(&self) -> &NonNull<u8> {
        &self.base
    }
}
