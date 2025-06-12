#![cfg_attr(feature = "nostd", no_std)]
//! # Custom Processing Unit
//! 
//! A crate for managing and interacting with GLM (Goldmont) processor microcode.
//! This crate provides functionality for patching, hooking, and manipulating the processor's
//! microcode at runtime.
//! 
//! ## Features
//! 
//! - `nostd`: Enables no_std compatibility for embedded/kernel environments
//! - `emulation`: Enables emulation mode for testing purposes
//! 
//! ## Acknowledgements
//! This crate is based on the work of [@pietroborrello](https://github.com/pietroborrello/CustomProcessingUnit)

use data_types::patch::UcodePatchEntry;

#[cfg(feature = "nostd")]
extern crate alloc;
#[cfg(feature = "nostd")]
use alloc::{format, string::String};
use data_types::addresses::UCInstructionAddress;

mod helpers;
pub use helpers::*;
use ucode_dump::{dump, RomDump};

pub mod patches;

/// Errors that can occur during microcode operations
#[derive(Debug)]
pub enum Error {
    /// The processor model is not supported
    InvalidProcessor(String),
    /// Failed to initialize match and patch functionality
    InitMatchAndPatchFailed(String),
    /// Failed to set up microcode hook
    HookFailed(String),
    /// Error during microcode patching operation
    PatchError(PatchError),
    /// Other unspecified errors
    Other(String),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::InvalidProcessor(t) => write!(f, "Unsupported GLM version: '{}'", t),
            Error::InitMatchAndPatchFailed(t) => {
                write!(f, "Failed to initialize match and patch: '{}'", t)
            }
            Error::HookFailed(t) => write!(f, "Failed to setup ucode hook: {}", t),
            Error::PatchError(t) => write!(f, "Failed to patch ucode: {:?}", t),
            Error::Other(t) => write!(f, "{}", t),
        }
    }
}

impl core::error::Error for Error {}

/// Result type for operations that can fail with a [`Error`]
pub type Result<T> = core::result::Result<T, Error>;

/// GLM processor version identifier for the old model
pub const GLM_OLD: u32 = 0x506c9;
/// GLM processor version identifier for the new model
pub const GLM_NEW: u32 = 0x506ca;

/// Main struct for managing processor microcode operations.
/// 
/// This struct provides functionality functions that are processor version specific.
pub struct CustomProcessingUnit {
    /// The current GLM processor version
    pub current_glm_version: u32,
}

impl CustomProcessingUnit {
    /// Creates a new instance of [`CustomProcessingUnit`].
    /// 
    /// This function detects the current GLM processor version and validates
    /// that it's supported. In emulation mode, it defaults to [`GLM_OLD`].
    /// 
    /// # Returns
    /// 
    /// - `Ok(CustomProcessingUnit)` if the processor is supported
    /// - `Err(Error::InvalidProcessor)` if the processor is not supported
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

    /// Initializes the microcode operations.
    /// 
    /// This function:
    /// 1. Activates debug instructions
    /// 2. Zeros out all hooks
    /// 3. Enables hooks globally
    pub fn init(&mut self) -> Result<()> {
        activate_udebug_insts();
        self.zero_hooks()?;
        enable_hooks();
        Ok(())
    }

    /// Applies existing microcode patches specific to the processor version.
    /// 
    /// For [`GLM_OLD`], this moves a patch from U7c5c to U7dfc.
    pub fn apply_existing_patches(&mut self) -> Result<()> {
        if self.current_glm_version == GLM_OLD {
            // Move the patch at U7c5c to U7dfc, since it seems important for the CPU
            const EXISTING_PATCH: [UcodePatchEntry; 1] = [
                // U7dfc: WRITEURAM(tmp5, 0x0037, 32) m2=1, NOP, NOP, SEQ_GOTO U60d2
                [0xa04337080235, 0, 0, 0x2460d200],
            ];
            patch_ucode(0x7dfc, &EXISTING_PATCH).map_err(Error::PatchError)?;
        }
        Ok(())
    }

    /// Uploads the zero hook function patch and returns its address.
    /// 
    /// # Returns
    /// 
    /// - `Ok(UCInstructionAddress)` with the address of the applied patch
    /// - `Err(Error)` if the patch application fails
    pub fn apply_zero_hook_func(&mut self) -> Result<UCInstructionAddress> {
        if self.current_glm_version == GLM_OLD {
            self.apply_existing_patches()?;

            // write and execute the patch that will zero out match&patch moving
            // the 0xc entry to last entry, which will make the hook call our moved patch
            let init_patch = patches::func_init::PATCH;
            patch_ucode(init_patch.addr, init_patch.ucode_patch).map_err(Error::PatchError)?;

            Ok(init_patch.addr)
        } else if self.current_glm_version == GLM_NEW {
            // write and execute the patch that will zero out match&patch
            let init_patch = patches::func_init_glm_new::PATCH;
            patch_ucode(init_patch.addr, init_patch.ucode_patch).map_err(Error::PatchError)?;

            Ok(init_patch.addr)
        } else {
            return Err(Error::InvalidProcessor(format!(
                "Unsupported GLM version: '{:08x}'",
                self.current_glm_version
            )));
        }
    }

    /// Executes the zero hooks function at the given address.
    /// 
    /// # Arguments
    /// 
    /// * `zero_hooks_func` - The address of the zero hooks function to execute
    /// 
    /// # Returns
    /// 
    /// - `Ok(())` if the function executes successfully
    /// - `Err(Error::InitMatchAndPatchFailed)` if the function fails
    pub fn zero_hooks_func(&mut self, zero_hooks_func: UCInstructionAddress) -> Result<()> {
        let result = call_custom_ucode_function(zero_hooks_func, [0; 3]);

        if result.rax != 0x0000133700001337 && cfg!(not(feature = "emulation")) {
            return Err(Error::InitMatchAndPatchFailed(format!(
                "invoke({}) = {:016x}, {:016x}, {:016x}, {:016x}",
                zero_hooks_func, result.rax, result.rbx, result.rcx, result.rdx
            )));
        }

        Ok(())
    }

    /// Zeros out all hook registers by first uploading then executing the zero hook function.
    pub fn zero_hooks(&mut self) -> Result<()> {
        let zero_func = self.apply_zero_hook_func()?;
        self.zero_hooks_func(zero_func)
    }

    /// Explicitly cleans up resources.
    /// 
    /// This is equivalent to dropping the instance, which will zero out all hooks.
    pub fn cleanup(self) {
        drop(self)
    }

    /// Returns a reference to the ROM dump for the current processor version.
    pub const fn rom(&self) -> &'static RomDump<'static, 'static> {
        match self.current_glm_version {
            GLM_OLD => &dump::ROM_cpu_000506C9,
            GLM_NEW => &dump::ROM_cpu_000506CA,
            _x => unreachable!(),
        }
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
