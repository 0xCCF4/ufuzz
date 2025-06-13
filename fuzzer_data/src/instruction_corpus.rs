//! Instruction corpus management
//!
//! This module provides types for managing a initial corpus of instructions used in fuzzing.

use alloc::collections::BTreeSet;
use alloc::vec::Vec;
use core::cmp::Ordering;
use serde::{Deserialize, Serialize};

/// A single instruction in the corpus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusInstruction {
    /// Raw instruction bytes
    pub bytes: Vec<u8>,
    /// Whether the instruction is valid
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

/// A collection of unique instructions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstructionCorpus {
    /// Set of unique instructions
    pub instructions: BTreeSet<CorpusInstruction>,
}
