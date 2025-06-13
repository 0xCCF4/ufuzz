//! # Coverage Library
//!
//! Coverage collection library, supporting no_std and std enviroments.
//!
//! ## Features
//!
//! This library provides several feature-gated modules:
//!
//! - `nostd`: Enables no_std compatibility
//! - `ucode`: Enables microcode compilation
//! - `uefi`: Provides UEFI-specific functionality
//! - `timing_measurement`: Enables performance timing measurement capabilities
//!
//! ## Modules
//!
//! - `patches`: Coverage collection implementation (requires `ucode` feature)
//! - `harness`: Coverage collection harness
//! - `interface`: Definitions to interact with collected coverage data
//! - `interface_definition`: Definitions of the coverage data structure
//! - `page_allocation`: UEFI-specific page allocation (requires `uefi` feature)
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
