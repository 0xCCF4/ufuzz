#![cfg_attr(feature = "no_std", no_std)]

use data_types::{UcodePatchEntry};

#[cfg(feature = "no_std")]
extern crate alloc;
#[cfg(feature = "no_std")]
use alloc::{format, string::String};

mod helpers;
pub use helpers::*;
pub mod labels;
pub mod patches;

#[derive(Debug)]
pub enum Error {
    InvalidProcessor(String),
    InitMatchAndPatchFailed(String),
    HookFailed(String),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::InvalidProcessor(t) => write!(f, "Unsupported GLM version: '{}'", t),
            Error::InitMatchAndPatchFailed(t) => {
                write!(f, "Failed to initialize match and patch: '{}'", t)
            }
            Error::HookFailed(t) => write!(f, "Failed to setup ucode hook: {}", t),
        }
    }
}

impl core::error::Error for Error {}

pub type Result<T> = core::result::Result<T, Error>;

pub const GLM_OLD: u32 = 0x506c9;
pub const GLM_NEW: u32 = 0x506ca;

pub struct CustomProcessingUnit {
    pub current_glm_version: u32,
}

impl CustomProcessingUnit {
    pub fn new() -> Result<CustomProcessingUnit> {
        let current_glm_version = detect_glm_version();

        if current_glm_version == GLM_OLD {
            Ok(CustomProcessingUnit {
                current_glm_version,
            })
        } else if current_glm_version == GLM_NEW {
            Ok(CustomProcessingUnit {
                current_glm_version,
            })
        } else {
            if cfg!(feature = "emulation") {
                return Ok(CustomProcessingUnit { current_glm_version: GLM_OLD});
            }

            Err(Error::InvalidProcessor(format!(
                "Unsupported GLM version: '{:08x}'",
                current_glm_version
            ))
            .into())
        }
    }

    pub fn init(&self) -> Result<()> {
        activate_udebug_insts();
        self.zero_hooks()?;
        enable_hooks();
        Ok(())
    }

    /// Zeros all hook registers.
    pub fn zero_hooks(&self) -> Result<()> {
        let result = if self.current_glm_version == GLM_OLD {
            // Move the patch at U7c5c to U7dfc, since it seems important for the CPU
            const EXISTING_PATCH: [UcodePatchEntry; 1] = [
                // U7dfc: WRITEURAM(tmp5, 0x0037, 32) m2=1, NOP, NOP, SEQ_GOTO U60d2
                [0xa04337080235, 0, 0, 0x2460d200],
            ];
            patch_ucode(0x7dfc, &EXISTING_PATCH);

            // write and execute the patch that will zero out match&patch moving
            // the 0xc entry to last entry, which will make the hook call our moved patch
            let init_patch = patches::func_init;
            patch_ucode(init_patch.addr, init_patch.ucode_patch);

            call_custom_ucode_function(init_patch.addr, [0; 3])
        } else if self.current_glm_version == GLM_NEW {
            // write and execute the patch that will zero out match&patch
            let init_patch = patches::func_init_glm_new;
            patch_ucode(init_patch.addr, init_patch.ucode_patch);

            call_custom_ucode_function(init_patch.addr, [0; 3])
        } else {
            return Err(Error::InvalidProcessor(format!(
                "Unsupported GLM version: '{:08x}'",
                self.current_glm_version
            ))
            .into());
        };

        if result.rax != 0x0000133700001337 && cfg!(not(feature = "emulation")) {
            return Err(Error::InitMatchAndPatchFailed(format!(
                "invoke(U{:04x}) = {:016x}, {:016x}, {:016x}, {:016x}",
                0x7da0, result.rax, result.rbx, result.rcx, result.rdx
            ))
                .into());
        }

        Ok(())
    }
}
