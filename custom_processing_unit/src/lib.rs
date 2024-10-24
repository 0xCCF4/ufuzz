mod arch;

use error_chain::error_chain;
use helpers::*;
use ucode_compiler::{UcodePatchBlob, UcodePatchEntry};

mod helpers;
mod patches;

error_chain! {
    errors {
        InvalidProcessor(t: String) {
            description("Invalid processor")
            display("Unsupported GLM version: '{}'", t)
        }
        InitMatchAndPatchFailed(t: String) {
            description("Failed to initialize match and patch")
            display("Failed to initialize match and patch: '{}'", t)
        }
    }

    skip_msg_variant
}

const GLM_OLD: u32 = 0x506c9;
const GLM_NEW: u32 = 0x506ca;

pub struct CustomProcessingUnit {
    pub current_glm_version: u32,
}

use crate::helpers::detect_glm_version;

fn patch_ucode(addr: usize, ucode_patch: &UcodePatchBlob) {
    // format: uop0, uop1, uop2, seqword
    // uop3 is fixed to a nop and cannot be overridden

    for i in 0..ucode_patch.len() {
        // patch ucode
        ms_patch_ram_write(
            crate::helpers::ucode_addr_to_patch_addr(addr + i * 4),
            ucode_patch[i][0],
        );
        ms_patch_ram_write(
            crate::helpers::ucode_addr_to_patch_addr(addr + i * 4) + 1,
            ucode_patch[i][1],
        );
        ms_patch_ram_write(
            crate::helpers::ucode_addr_to_patch_addr(addr + i * 4) + 2,
            ucode_patch[i][2],
        );

        // patch seqword
        ms_const_write(
            crate::helpers::ucode_addr_to_patch_seqword_addr(addr) + i,
            ucode_patch[i][3],
        );
    }
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
            Err(ErrorKind::InvalidProcessor(format!(
                "Unsupported GLM version: '{:08x}'",
                current_glm_version
            ))
            .into())
        }
    }

    pub fn init(&self) {
        setup_exceptions();
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

    pub fn init_match_and_patch(&self) -> Result<()> {
        if self.current_glm_version == GLM_OLD {
            // Move the patch at U7c5c to U7dfc, since it seems important for the CPU
            const existing_patch: [UcodePatchEntry; 1] = [
                // U7dfc: WRITEURAM(tmp5, 0x0037, 32) m2=1, NOP, NOP, SEQ_GOTO U60d2
                [0xa04337080235, 0, 0, 0x2460d200],
            ];
            patch_ucode(0x7dfc, &existing_patch);

            // write and execute the patch that will zero out match&patch moving
            // the 0xc entry to last entry, which will make the hook call our moved patch
            let init_patch = patches::match_patch_init;
            patch_ucode(init_patch.addr, init_patch.ucode_patch);

            let mut resA = 0;
            let mut resB = 0;
            let mut resC = 0;
            let mut resD = 0;
            udebug_invoke(init_patch.addr, &mut resA, &mut resB, &mut resC, &mut resD);
            if resA != 0x0000133700001337 {
                return Err(ErrorKind::InitMatchAndPatchFailed(format!(
                    "invoke({:08x}) = {:016x}, {:016x}, {:016x}, {:016x}",
                    init_patch.addr, resA, resB, resC, resD
                ))
                .into());
            }
        } else if self.current_glm_version == GLM_NEW {
            // write and execute the patch that will zero out match&patch
            let init_patch = patches::match_patch_init_glm_new;
            patch_ucode(init_patch.addr, init_patch.ucode_patch);

            let mut resA = 0;
            let mut resB = 0;
            let mut resC = 0;
            let mut resD = 0;
            udebug_invoke(init_patch.addr, &mut resA, &mut resB, &mut resC, &mut resD);
            if resA != 0x0000133700001337 {
                return Err(ErrorKind::InitMatchAndPatchFailed(format!(
                    "invoke({:08x}) = {:016x}, {:016x}, {:016x}, {:016x}",
                    init_patch.addr, resA, resB, resC, resD
                ))
                .into());
            }
        } else {
            return Err(ErrorKind::InvalidProcessor(format!(
                "Unsupported GLM version: '{:08x}'",
                self.current_glm_version
            ))
            .into());
        }
        self.enable_match_and_patch();

        Ok(())
    }
}
