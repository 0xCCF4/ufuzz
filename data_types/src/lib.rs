//! # Data Types for Microcode Operations
//! 
//! This crate provides fundamental data types for working with processor microcode.
//! It supports both standard and no_std environments through feature flags.
//! 
//! ## Features
//! 
//! - `nostd`: Enables no_std compatibility
//! 
//! ## Modules
//! 
//! - [`addresses`]: Types for representing and manipulating various microcode addresses
//! - [`patch`]: Types for representing microcode patches and triads
#![cfg_attr(feature = "nostd", no_std)]

extern crate alloc;
#[cfg(feature = "nostd")]
extern crate core;

#[cfg(feature = "nostd")]
mod nostd;
#[cfg(feature = "nostd")]
use nostd as cstd;
#[cfg(not(feature = "nostd"))]
use std as cstd;

pub mod addresses;
pub mod patch;
