#![cfg_attr(feature = "no_std", no_std)]
extern crate alloc;

#[cfg(feature = "std")]
pub mod compiler_call;
pub mod utils;
