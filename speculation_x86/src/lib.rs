//! # Speculation X86
//!
//! An experiment for analyzing and testing x86 architecture-level speculation behavior.
#![no_std]

use x86_perf_counter::PerfEventSpecifier;

pub mod patches;
