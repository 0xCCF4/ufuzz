#![no_std]

extern crate core;

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq)]
pub struct PerfEventSpecifier {
    pub event_select: u8,
    pub umask: u8,
    pub edge_detect: Option<u8>,
    pub cmask: Option<u8>,
}

impl AsRef<PerfEventSpecifier> for PerfEventSpecifier {
    fn as_ref(&self) -> &PerfEventSpecifier {
        &self
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
mod x86;

use serde::{Deserialize, Serialize};
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
pub use x86::*;