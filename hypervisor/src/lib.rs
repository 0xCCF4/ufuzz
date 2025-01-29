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

use alloc::boxed::Box;
use x86::bits64::paging::BASE_PAGE_SIZE;

pub mod hardware_vt;

pub mod x86_instructions;

pub mod vm;

pub mod error;

pub use error::HypervisorError as Error;
pub use error::Result;

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
    pub fn as_slice_mut(&mut self) -> &mut [u8] {
        &mut self.0
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    pub fn as_ptr(&self) -> *const Page {
        self as *const Page
    }

    pub fn as_mut_ptr(&mut self) -> *mut Page {
        self as *mut Page
    }

    pub fn alloc_zeroed() -> Box<Self> {
        unsafe { Box::<Page>::new_zeroed().assume_init() }
    }

    pub fn fill(&mut self, value: u8) {
        self.as_slice_mut().fill(value);
    }

    pub fn zero(&mut self) {
        self.fill(0);
    }
}

const _: () = assert!(size_of::<Page>() == 0x1000);
