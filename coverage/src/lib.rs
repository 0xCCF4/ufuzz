#![cfg_attr(feature = "nostd", no_std)]

#[cfg(feature = "ucode")]
mod patches;
#[cfg(feature = "ucode")]
pub use patches::helpers as coverage_collector_debug_tools;
#[cfg(feature = "ucode")]
pub use patches::patch as coverage_collector;
pub mod harness;
#[cfg(feature = "ucode")]
pub mod interface;
pub mod interface_definition;

#[cfg(feature = "uefi")]
pub mod page_allocation;

//#[cfg(feature = "nostd")]
extern crate alloc;
