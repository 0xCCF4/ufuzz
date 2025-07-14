use alloc::string::String;
use alloc::vec::Vec;
#[cfg(feature = "clap")]
use clap::Parser;
use serde::{Deserialize, Serialize};

pub const NAME: &str = "SPECTRE";

/// Type of fencing to apply
#[cfg_attr(feature = "clap", derive(Parser))]
#[derive(Debug, Clone, Serialize, Deserialize, Default, Copy, PartialEq, Eq)]
pub enum FenceType {
    /// No fencing instruction after executing
    #[default]
    None,
    /// Use CPUID to fence
    CPUID,
    /// Use modified microcode implementation to fence
    MICRO,
}

#[cfg_attr(feature = "clap", derive(Parser))]
/// The subcommand for this scenario
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Payload {
    /// Prepares the experiment setup
    Reset,
    /// Place microcode synchronization
    Sync,
    /// Remove microcode synchronization
    NoSync,
    /// Proof that mispredicted branch is never executed
    Test,
    /// Executes experiment #F1, showing uSpectre with fencing (None, CPUID, MICRO)
    #[cfg_attr(feature = "clap", command(subcommand))]
    Execute(FenceType),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ResultingData {
    Error(String),
    Ok,
    CacheTimings(Vec<u64>),
    TestResult(TestResult),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum TestResult {
    Ok,
    BranchWasNotTakenWhoops,
    MarkerMissing,
}
