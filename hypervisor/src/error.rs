//! Error Handling Module
//!
//! This module defines error types and result aliases used throughout the hypervisor.
//! It provides a standardized way to handle and propagate errors in virtualization operations.

use thiserror_no_std::Error;

/// Errors that can occur during hypervisor operations
#[derive(Error, Debug)]
pub enum HypervisorError {
    /// Indicates that the guest VM has exhausted all available, preallocated page tables. Limit set in code, maybe increase?
    #[error("Guest VM ran out of LDT entries")]
    NestedPagingStructuresExhausted,

    /// Indicates a failure to enable virtualization features on the host CPU
    #[error("Failed to enable virtualization on the CPU")]
    FailedToInitializeHost(&'static str),
}

/// A type alias for `Result<T, HypervisorError>`
///
/// This is the standard result type used throughout the hypervisor for operations
/// that may fail with a `HypervisorError`.
pub type Result<T> = core::result::Result<T, HypervisorError>;
