#![cfg_attr(feature = "nostd", no_std)]

mod patches;
pub use patches::helpers as coverage_collector_debug_tools;
pub use patches::patch as coverage_collector;
pub mod harness;
pub mod interface;
pub mod interface_definition;

#[cfg(feature = "uefi")]
pub mod page_allocation;

//#[cfg(feature = "nostd")]
extern crate alloc;
