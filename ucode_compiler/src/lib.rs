#![cfg_attr(feature = "no_std", no_std)]
extern crate alloc;

#[cfg(all(feature = "uasm", not(feature = "no_std")))]
pub mod uasm;
pub mod utils;
