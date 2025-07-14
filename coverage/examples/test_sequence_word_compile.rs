#![no_main]
#![no_std]

//! Runtime test for the runtime sequence word compiler.

use custom_processing_unit::{apply_patch, call_custom_ucode_function, CustomProcessingUnit};
use log::info;
use ucode_compiler_dynamic::sequence_word::SequenceWord;
use uefi::{entry, print, println, Status};

mod patch {
    use ucode_compiler_derive::patch;

    patch!(
        .org 0x7c00

        <func_test>
        # IN: tmp0 = address
        func compute_exit_jump(rax, tmp0, tmp1, tmp2)
    );
}

#[entry]
unsafe fn main() -> Status {
    uefi::helpers::init().unwrap();

    let mut cpu = match CustomProcessingUnit::new() {
        Ok(cpu) => cpu,
        Err(e) => {
            info!("Failed to initiate program {:?}", e);
            return Status::ABORTED;
        }
    };
    if let Err(e) = cpu.init() {
        info!("Failed to initiate program {:?}", e);
        return Status::ABORTED;
    }

    if let Err(e) = cpu.zero_hooks() {
        info!("Failed to zero hooks {:?}", e);
        return Status::ABORTED;
    }

    if let Err(err) = apply_patch(&patch::PATCH) {
        println!("Failed to apply patch: {:?}", err);
        return Status::ABORTED;
    }

    println!("Testing sequence words");
    let mut diff = -1;
    for i in 0..0x7e00 {
        if i % 0x100 == 0 {
            print!("\r[{:04x}] ", i);
        }

        let seqw = SequenceWord::new().set_goto(0, i).assemble().unwrap();
        let calculation = call_custom_ucode_function(patch::LABEL_FUNC_TEST, [i, 0, 0]).rax;

        if diff == 0 {
            break;
        } else if diff > 0 {
            diff -= 1;
            println!(
                "\r            \n{:04x}: {:08x}    {:08x}",
                i, seqw, calculation
            );
        } else if seqw != calculation as u32 {
            println!(
                "\r            \n{:04x}: {:08x} != {:08x}",
                i, seqw, calculation
            );
            diff = 5;
        }
    }
    if diff == -1 {
        println!("OK");
        let _ = cpu.zero_hooks();
        Status::SUCCESS
    } else {
        println!("Failed");
        let _ = cpu.zero_hooks();
        Status::ABORTED
    }
}
