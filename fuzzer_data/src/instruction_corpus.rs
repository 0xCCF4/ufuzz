use alloc::collections::BTreeSet;
use alloc::vec::Vec;
use core::cmp::Ordering;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusInstruction {
    pub bytes: Vec<u8>,
    pub valid: bool,
}

impl Ord for CorpusInstruction {
    fn cmp(&self, other: &Self) -> Ordering {
        self.bytes.cmp(&other.bytes)
    }
}

impl PartialOrd for CorpusInstruction {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.bytes.partial_cmp(&other.bytes)
    }
}

impl PartialEq for CorpusInstruction {
    fn eq(&self, other: &Self) -> bool {
        self.bytes == other.bytes
    }
}

impl Eq for CorpusInstruction {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstructionCorpus {
    pub instructions: BTreeSet<CorpusInstruction>,
}
