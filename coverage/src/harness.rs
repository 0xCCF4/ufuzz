//! This module provides different types of harnesses for coverage collection and analysis:
//!
//! - `coverage_harness`: Core functionality for collecting code coverage information
//! - `iteration_harness`: Tools for iteratively calling the coverage harness for the entire microcode address space
//! - `validation_harness`: Deprecated
//!

pub mod coverage_harness;
pub mod iteration_harness;
#[cfg(feature = "ucode")]
pub mod validation_harness;
