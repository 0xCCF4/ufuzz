#![cfg_attr(feature = "no_std", no_std)]

use data_types::UcodePatchEntry;

#[cfg(feature = "no_std")]
extern crate alloc;
#[cfg(feature = "no_std")]
use alloc::{format, string::String};
use data_types::addresses::UCInstructionAddress;

mod helpers;
pub use helpers::*;
pub mod dump;
pub mod opcodes;
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

// Will zero out all hooks on dropping
pub struct CustomProcessingUnit {
    pub current_glm_version: u32,
}

impl CustomProcessingUnit {
    pub fn new() -> Result<CustomProcessingUnit> {
        let current_glm_version = detect_glm_version();

        if matches!(current_glm_version, GLM_OLD | GLM_NEW) {
            Ok(CustomProcessingUnit {
                current_glm_version,
            })
        } else {
            if cfg!(feature = "emulation") {
                return Ok(CustomProcessingUnit {
                    current_glm_version: GLM_OLD,
                });
            }

            Err(Error::InvalidProcessor(format!(
                "Unsupported GLM version: '{:08x}'",
                current_glm_version
            )))
        }
    }

    pub fn init(&self) -> Result<()> {
        activate_udebug_insts();
        self.zero_hooks()?;
        enable_hooks();
        Ok(())
    }

    pub fn apply_existing_patches(&self) {
        if self.current_glm_version == GLM_OLD {
            // Move the patch at U7c5c to U7dfc, since it seems important for the CPU
            const EXISTING_PATCH: [UcodePatchEntry; 1] = [
                // U7dfc: WRITEURAM(tmp5, 0x0037, 32) m2=1, NOP, NOP, SEQ_GOTO U60d2
                [0xa04337080235, 0, 0, 0x2460d200],
            ];
            patch_ucode(0x7dfc, &EXISTING_PATCH);
        }
    }

    pub fn apply_zero_hook_func(&self) -> Result<UCInstructionAddress> {
        if self.current_glm_version == GLM_OLD {
            self.apply_existing_patches();

            // write and execute the patch that will zero out match&patch moving
            // the 0xc entry to last entry, which will make the hook call our moved patch
            let init_patch = patches::func_init::PATCH;
            patch_ucode(init_patch.addr, init_patch.ucode_patch);

            Ok(init_patch.addr)
        } else if self.current_glm_version == GLM_NEW {
            // write and execute the patch that will zero out match&patch
            let init_patch = patches::func_init_glm_new::PATCH;
            patch_ucode(init_patch.addr, init_patch.ucode_patch);

            Ok(init_patch.addr)
        } else {
            return Err(Error::InvalidProcessor(format!(
                "Unsupported GLM version: '{:08x}'",
                self.current_glm_version
            )));
        }
    }

    /// Zeros all hook registers.
    pub fn zero_hooks_func(&self, zero_hooks_func: UCInstructionAddress) -> Result<()> {
        let result = call_custom_ucode_function(zero_hooks_func, [0; 3]);

        if result.rax != 0x0000133700001337 && cfg!(not(feature = "emulation")) {
            return Err(Error::InitMatchAndPatchFailed(format!(
                "invoke({}) = {:016x}, {:016x}, {:016x}, {:016x}",
                zero_hooks_func, result.rax, result.rbx, result.rcx, result.rdx
            )));
        }

        Ok(())
    }

    pub fn zero_hooks(&self) -> Result<()> {
        self.zero_hooks_func(self.apply_zero_hook_func()?)
    }

    pub fn cleanup(self) {
        drop(self)
    }
}

impl Drop for CustomProcessingUnit {
    fn drop(&mut self) {
        match self.zero_hooks() {
            Ok(_) => {}
            Err(e) => {
                log::error!("Failed to zero hooks: {}", e);
            }
        }
    }
}
