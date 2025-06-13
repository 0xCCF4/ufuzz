//! # Ucode Compiler Bridge
//!
//! A crate that provides an interface between Rust and Python assembler for microcode compilation.
extern crate alloc;
extern crate core;

#[allow(clippy::bind_instead_of_map)] // todo: if time, then remove this line
mod uasm;

pub use uasm::*;
