use alloc::format;
use alloc::string::String;
use core::arch::asm;
use custom_processing_unit::{CustomProcessingUnit, apply_hook_patch_func, apply_patch, hook};
use data_types::addresses::MSRAMHookIndex;
use itertools::Itertools;
use log::{error, trace};
use poc_data::f0_microcode::{Payload, ResultingData};

pub fn execute(payload: Payload) -> ResultingData {
    trace!("[F0]: {payload:?}");
    match payload {
        Payload::Reset => reset()
            .map(ResultingData::Error)
            .unwrap_or(ResultingData::Ok),
        Payload::Enable => enable(true)
            .map(ResultingData::Error)
            .unwrap_or(ResultingData::Ok),
        Payload::Disable => {
            enable(false);
            ResultingData::Ok
        }
        Payload::Random => ResultingData::RandomNumbers(
            [0..16]
                .iter()
                .map(|_| (rdrand().1 % (u8::MAX as u64)) as u8)
                .collect_vec(),
        ),
    }
}

fn enable(enabled: bool) -> Option<String> {
    if let Err(err) = hook(
        apply_hook_patch_func(),
        MSRAMHookIndex::ZERO,
        ucode_dump::dump::cpu_000506CA::RDRAND_XLAT,
        patch::LABEL_POC,
        enabled,
    ) {
        error!("Failed to apply hook: {}", err);
        Some(format!("Failed to apply hook: {}", err))
    } else {
        None
    }
}

fn reset() -> Option<String> {
    let mut cpu = match CustomProcessingUnit::new() {
        Ok(cpu) => cpu,
        Err(e) => {
            error!("Failed to initiate program {:?}", e);
            return Some(format!("Failed to initiate program {:?}", e));
        }
    };
    if let Err(e) = cpu.init() {
        error!("Failed to initiate program {:?}", e);
        return Some(format!("Failed to initiate program {:?}", e));
    }

    if let Err(e) = cpu.zero_hooks() {
        error!("Failed to zero hooks {:?}", e);
        return Some(format!("Failed to zero hooks {:?}", e));
    }

    if let Err(err) = apply_patch(&patch::PATCH) {
        error!("Failed to apply patch {:?}", err);
        return Some(format!("Failed to apply patch {:?}", err));
    }

    None
}

mod patch {
    use ucode_compiler_derive::patch;

    patch!(
        .org 0x7c00

        <POC>
        rax := ZEROEXT_DSZ64(0x0007)
    );
}

fn rdrand() -> (bool, u64) {
    let flags: u8;
    let result: u64;
    unsafe {
        asm! {
        "rdrand rax",
        "setc {flags}",
        flags = out(reg_byte) flags,
        out("rax") result,
        options(nostack),
        }
    }
    (flags > 0, result)
}
