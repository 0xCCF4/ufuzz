//! Types for representing microcode patches
//! 
//! This module provides types for working with microcode patches, including:
//! - Patch entries (triads of instructions)
//! - Patch blobs (collections of patch entries)
//! - Label mappings for symbolic references
//! - Microcode patches (collection of blobs and labels)

use crate::addresses::{MSRAMHookIndex, UCInstructionAddress};

/// A single microcode patch entry consisting of 4 values:
/// - 3 instructions (uop0, uop1, uop2)
/// - 1 sequence word
pub type UcodePatchEntry = [usize; 4];

/// A collection of microcode patch entries
pub type UcodePatchBlob = [UcodePatchEntry];

/// A mapping between a symbolic label and its instruction address
pub type LabelMapping<'a> = (&'a str, UCInstructionAddress);

/// Configuration for a microcode patch
pub struct Patch<'a, 'b, 'c> {
    /// The address where the patch should be applied
    pub addr: UCInstructionAddress,
    /// Optional address to hook (redirect execution from)
    pub hook_address: Option<UCInstructionAddress>,
    /// Optional index for the hook in the MSRAM
    pub hook_index: Option<MSRAMHookIndex>,
    /// The actual patch content
    pub ucode_patch: &'a UcodePatchBlob,
    /// Label mappings for symbolic references in the patch
    pub labels: &'b [LabelMapping<'c>],
}

/// A triad of microcode instructions with a sequence word
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Triad {
    /// Three 64-bit instructions
    pub instructions: [u64; 3],
    /// 32-bit sequence word
    pub sequence_word: u32,
}
