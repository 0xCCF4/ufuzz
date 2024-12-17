use crate::addresses::{MSRAMHookIndex, UCInstructionAddress};

pub type UcodePatchEntry = [usize; 4];
pub type UcodePatchBlob = [UcodePatchEntry];
pub type LabelMapping<'a> = (&'a str, UCInstructionAddress);

// todo use triads
pub struct Patch<'a, 'b, 'c> {
    pub addr: UCInstructionAddress,
    pub hook_address: Option<UCInstructionAddress>,
    pub hook_index: Option<MSRAMHookIndex>,
    pub ucode_patch: &'a UcodePatchBlob,
    pub labels: &'b [LabelMapping<'c>],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Triad {
    pub instructions: [u64; 3],
    pub sequence_word: u32,
}
