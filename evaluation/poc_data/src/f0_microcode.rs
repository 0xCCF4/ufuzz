use alloc::string::String;
use alloc::vec::Vec;
#[cfg(feature = "clap")]
use clap::Parser;
use serde::{Deserialize, Serialize};

pub const NAME: &str = "MICROCODE";

#[cfg_attr(feature = "clap", derive(Parser))]
/// The subcommand for this scenario
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Payload {
    /// Zeros hooks and upload microcode update
    Reset,
    /// Generate random numbers
    Random,
    /// Enable microcode hooks
    Enable,
    /// Disable microcode hooks
    Disable,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ResultingData {
    Error(String),
    Ok,
    RandomNumbers(Vec<u8>),
}
