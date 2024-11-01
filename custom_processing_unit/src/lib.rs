#![cfg_attr(feature = "no_std", no_std)]

use data_types::{Patch, UcodePatchEntry};

#[cfg(feature = "no_std")]
extern crate alloc;
#[cfg(feature = "no_std")]
use alloc::{format, string::String};
use data_types::addresses::{MSRAMHookAddress, UCInstructionAddress};

mod helpers;
pub use helpers::*;
pub mod labels;
mod patches;

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

const GLM_OLD: u32 = 0x506c9;
const GLM_NEW: u32 = 0x506ca;

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
            Err(Error::InvalidProcessor(format!(
                "Unsupported GLM version: '{:08x}'",
                current_glm_version
            ))
            .into())
        }
    }

    pub fn init(&self) {
        // setup_exceptions();
        activate_udebug_insts();
        self.enable_match_and_patch();
    }

    pub fn enable_match_and_patch(&self) {
        let mp = crbus_read(0x692);
        crbus_write(0x692, mp & !1usize);
    }

    pub fn disable_match_and_patch(&self) {
        let mp = crbus_read(0x692);
        crbus_write(0x692, mp | 1usize);
    }

    pub fn patch(&self, patch: &Patch) {
        patch_ucode(patch.addr, patch.ucode_patch);
    }

    pub fn hook_patch(&self, patch: &Patch) -> Result<()> {
        if let Some(hook_address) = patch.hook_address {
            let hook_index = patch.hook_index.unwrap_or(MSRAMHookAddress::ZERO);

            self.hook(hook_index, hook_address, patch.addr)
        } else {
            Err(Error::HookFailed(
                "No hook address present in patch.".into(),
            ))
        }
    }

    pub fn hook(
        &self,
        hook_idx: MSRAMHookAddress,
        uop_address: UCInstructionAddress,
        patch_address: UCInstructionAddress,
    ) -> Result<()> {
        hook_match_and_patch(hook_idx, uop_address, patch_address)
    }

    pub fn zero_match_and_patch(&self) -> Result<()> {
        self.init_match_and_patch()
    }

    pub fn init_match_and_patch(&self) -> Result<()> {
        if self.current_glm_version == GLM_OLD {
            // Move the patch at U7c5c to U7dfc, since it seems important for the CPU
            const EXISTING_PATCH: [UcodePatchEntry; 1] = [
                // U7dfc: WRITEURAM(tmp5, 0x0037, 32) m2=1, NOP, NOP, SEQ_GOTO U60d2
                [0xa04337080235, 0, 0, 0x2460d200],
            ];
            patch_ucode(0x7dfc, &EXISTING_PATCH);

            // write and execute the patch that will zero out match&patch moving
            // the 0xc entry to last entry, which will make the hook call our moved patch
            let init_patch = patches::match_patch_init;
            patch_ucode(init_patch.addr, init_patch.ucode_patch);

            let mut res_a = 0;
            let mut res_b = 0;
            let mut res_c = 0;
            let mut res_d = 0;
            udebug_invoke(
                init_patch.addr,
                &mut res_a,
                &mut res_b,
                &mut res_c,
                &mut res_d,
            );
            if res_a != 0x0000133700001337 {
                return Err(Error::InitMatchAndPatchFailed(format!(
                    "invoke({}) = {:016x}, {:016x}, {:016x}, {:016x}",
                    init_patch.addr, res_a, res_b, res_c, res_d
                ))
                .into());
            }
        } else if self.current_glm_version == GLM_NEW {
            // write and execute the patch that will zero out match&patch
            let init_patch = patches::match_patch_init_glm_new;
            patch_ucode(init_patch.addr, init_patch.ucode_patch);

            let mut res_a = 0;
            let mut res_b = 0;
            let mut res_c = 0;
            let mut res_d = 0;
            udebug_invoke(
                init_patch.addr,
                &mut res_a,
                &mut res_b,
                &mut res_c,
                &mut res_d,
            );
            if res_a != 0x0000133700001337 {
                return Err(Error::InitMatchAndPatchFailed(format!(
                    "invoke({}) = {:016x}, {:016x}, {:016x}, {:016x}",
                    init_patch.addr, res_a, res_b, res_c, res_d
                ))
                .into());
            }
        } else {
            return Err(Error::InvalidProcessor(format!(
                "Unsupported GLM version: '{:08x}'",
                self.current_glm_version
            ))
            .into());
        }
        self.enable_match_and_patch();

        Ok(())
    }
}
