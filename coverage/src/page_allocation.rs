use core::mem;
use core::ptr::NonNull;
use uefi::boot::{AllocateType, MemoryType};
use uefi::data_types::PhysicalAddress;

pub struct PageAllocation {
    base: NonNull<u8>,
    count: usize,
}

impl Drop for PageAllocation {
    fn drop(&mut self) {
        unsafe { uefi::boot::free_pages(self.base, self.count).expect("Unable to free pages") }
    }
}

impl PageAllocation {
    pub fn alloc_address(address: PhysicalAddress, count: usize) -> uefi::Result<PageAllocation> {
        let data = uefi::boot::allocate_pages(
            AllocateType::Address(address),
            MemoryType::LOADER_DATA,
            count,
        )?;
        Ok(PageAllocation { count, base: data })
    }

    pub fn alloc(count: usize) -> uefi::Result<PageAllocation> {
        let data = uefi::boot::allocate_pages(AllocateType::AnyPages, MemoryType::LOADER_DATA, count)?;
        Ok(PageAllocation { count, base: data })
    }
    #[allow(unused)]
    pub fn ptr(&self) -> &NonNull<u8> {
        &self.base
    }
    pub fn dealloc(self) {
        drop(self)
    }

    pub fn address(&self) -> usize {
        unsafe { mem::transmute(self.base.cast::<()>()) }
    }
}

impl AsRef<NonNull<u8>> for PageAllocation {
    fn as_ref(&self) -> &NonNull<u8> {
        &self.base
    }
}
