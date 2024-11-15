#![cfg_attr(feature = "no_std", no_std)]
extern crate alloc;

#[cfg(feature = "uasm")]
pub mod uasm;
pub mod utils;
