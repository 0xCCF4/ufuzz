#![cfg_attr(feature = "nostd", no_std)]

extern crate alloc;
#[cfg(feature = "nostd")]
extern crate core;

#[cfg(feature = "nostd")]
mod nostd;
#[cfg(feature = "nostd")]
use nostd as cstd;
#[cfg(not(feature = "nostd"))]
use std as cstd;

pub mod addresses;
pub mod patch;


