#![cfg_attr(feature = "no_std", no_std)]
extern crate alloc;
extern crate core;
use core::fmt;

use core::fmt::{Display, Formatter};

#[cfg(all(feature = "uasm", not(feature = "no_std")))]
#[allow(clippy::bind_instead_of_map)] // todo: if time, then remove this line
pub mod uasm;
pub mod utils;

impl Display for utils::opcodes::Opcode {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}
