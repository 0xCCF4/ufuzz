#![cfg_attr(feature = "no_std", no_std)]

mod patches;
pub use patches::patch as coverage_collector;
pub mod coverage_harness;
pub mod interface;
pub mod interface_definition;

#[cfg(feature = "uefi")]
pub mod page_allocation;

#[cfg(feature = "no_std")]
extern crate alloc;

mod ms_array;
pub mod firmware {
    pub use super::ms_array::MS_ARRAY_0 as InstructionROM;
    pub use super::ms_array::MS_ARRAY_1 as SequenceWordROM;
}
