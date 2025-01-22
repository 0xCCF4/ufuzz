//! This crate provides an interface to a hypervisor.

#![no_std]
/*
#![feature(allocator_api)]
#![feature(const_trait_impl)]
#![feature(const_mut_refs)]
#![feature(naked_functions)]
#![feature(once_cell_try)]
#![feature(decl_macro)]
*/
#![feature(new_zeroed_alloc)]

extern crate alloc;

use x86::bits64::paging::{BASE_PAGE_SHIFT, BASE_PAGE_SIZE};

pub mod hardware_vt;

pub mod x86_instructions;

pub mod vm;

pub mod error;

pub use error::HypervisorError as Error;
pub use error::Result as Result;

pub mod hypervisor;

pub mod state;

pub mod x86_data;

/// The structure representing a single memory page (4KB).
//
// This does not _always_ have to be allocated at the page aligned address, but
// very often it is, so let us specify the alignment.
#[derive(Debug, Clone, Copy)]
#[repr(C, align(4096))]
pub struct Page([u8; BASE_PAGE_SIZE]);

impl Page {
    pub unsafe fn as_slice_mut(&mut self) -> &mut [u8] {
        &mut self.0
    }

    pub unsafe fn as_slice(&self) -> &[u8] {
        &self.0
    }

    pub fn as_ptr(&self) -> *const Page {
        self as *const Page
    }
}

const _: () = assert!(size_of::<Page>() == 0x1000);

/// Computes how many pages are needed for the given bytes.
fn size_to_pages(size: usize) -> usize {
    const PAGE_MASK: usize = 0xfff;

    (size >> BASE_PAGE_SHIFT) + usize::from((size & PAGE_MASK) != 0)
}