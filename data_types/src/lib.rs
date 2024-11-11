#![cfg_attr(feature = "nostd", no_std)]

use crate::addresses::{MSRAMHookIndex, UCInstructionAddress};

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

pub type UcodePatchEntry = [usize; 4];
pub type UcodePatchBlob = [UcodePatchEntry];
pub type LabelMapping<'a> = (&'a str, UCInstructionAddress);

pub struct Patch<'a, 'b, 'c> {
    pub addr: UCInstructionAddress,
    pub hook_address: Option<UCInstructionAddress>,
    pub hook_index: Option<MSRAMHookIndex>,
    pub ucode_patch: &'a UcodePatchBlob,
    pub labels: &'b [LabelMapping<'c>],
}
