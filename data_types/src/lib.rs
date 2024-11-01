#![cfg_attr(feature = "nostd", no_std)]

use crate::addresses::{MSRAMHookAddress, UCInstructionAddress};

extern crate alloc;
#[cfg(feature = "nostd")]
extern crate core;

#[cfg(feature = "nostd")]
mod nostd;
#[cfg(feature = "nostd")]
use nostd as std;
#[cfg(not(feature = "nostd"))]
use std;

pub mod addresses;

pub type UcodePatchEntry = [usize; 4];
pub type UcodePatchBlob = [UcodePatchEntry];

pub struct Patch<'a> {
    pub addr: UCInstructionAddress,
    pub hook_address: Option<UCInstructionAddress>,
    pub hook_index: Option<MSRAMHookAddress>,
    pub ucode_patch: &'a UcodePatchBlob,
}
