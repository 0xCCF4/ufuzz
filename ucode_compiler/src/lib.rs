#![cfg_attr(feature = "nostd", no_std)]
extern crate alloc;
extern crate core;

use core::fmt;

use core::fmt::{Display, Formatter};

#[cfg(all(feature = "uasm", not(feature = "nostd")))]
#[allow(clippy::bind_instead_of_map)] // todo: if time, then remove this line
pub mod uasm;
pub mod utils;

impl Display for utils::opcodes::Opcode {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}
