#![no_main]
#![no_std]

//! Runtime test for the coverage collector patch (setup) ucode function.

extern crate alloc;

use coverage::interface::safe::ComInterface;
use coverage::interface_definition::InstructionTableEntry;
use coverage::{coverage_collector, interface_definition};
use custom_processing_unit::{
    apply_patch, call_custom_ucode_function, disable_all_hooks, ms_patch_instruction_read,
    ms_seqw_read, CustomProcessingUnit,
};
use data_types::addresses::UCInstructionAddress;
use log::info;
use uefi::prelude::*;
use uefi::{print, println};

#[entry]
unsafe fn main() -> Status {
    uefi::helpers::init().unwrap();
    info!("Hello world!");

    let cpu = match CustomProcessingUnit::new() {
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

    let mut interface = match ComInterface::new(&interface_definition::COM_INTERFACE_DESCRIPTION) {
        Ok(interface) => interface,
        Err(e) => {
            info!("Failed to initiate program {:?}", e);
            return Status::ABORTED;
        }
    };
    let hooks = {
        let max_hooks = interface.description().max_number_of_hooks;

        let device_max_hooks = match cpu.current_glm_version {
            custom_processing_unit::GLM_OLD => 31,
            custom_processing_unit::GLM_NEW => 32,
            _ => 0,
        };

        max_hooks.min(device_max_hooks)
    };

    if hooks == 0 {
        info!("No hooks available");
        return Status::ABORTED;
    }

    interface.reset_coverage();

    apply_patch(&coverage_collector::PATCH);

    fn print_status() {
        print!("Hooks  : ");
        for i in 0..interface_definition::COM_INTERFACE_DESCRIPTION.max_number_of_hooks as usize {
            let hook = call_custom_ucode_function(
                coverage_collector::LABEL_FUNC_LDAT_READ_HOOKS,
                [i, 0, 0],
            )
            .rax;
            print!("{:08x}, ", hook);
        }
        println!();

        for i in 0..interface_definition::COM_INTERFACE_DESCRIPTION.max_number_of_hooks as usize {
            print!("EXIT {:02}: ", i);
            let seqw = ms_seqw_read(
                coverage_collector::LABEL_FUNC_LDAT_READ,
                coverage_collector::LABEL_HOOK_EXIT_00 + i * 4,
            );
            print!("{:08x} -> ", seqw);
            for offset in 0..3 {
                let instruction = ms_patch_instruction_read(
                    coverage_collector::LABEL_FUNC_LDAT_READ,
                    coverage_collector::LABEL_HOOK_EXIT_REPLACEMENT_00 + i * 4 + offset,
                );
                print!("{:08x} ", instruction);
            }
            let seqw = ms_seqw_read(
                coverage_collector::LABEL_FUNC_LDAT_READ,
                coverage_collector::LABEL_HOOK_EXIT_REPLACEMENT_00 + i * 4,
            );
            println!("-> {:08x}", seqw);
        }
    }

    unsafe fn setup_hooks(
        interface: &mut ComInterface,
        addresses: &[UCInstructionAddress],
        instructions: &[InstructionTableEntry],
        printing: bool,
    ) {
        assert_eq!(addresses.len(), instructions.len());
        interface.write_jump_table_all(addresses);
        interface.write_instruction_table_all(instructions.iter().cloned().into_iter());

        let result = call_custom_ucode_function(
            coverage_collector::LABEL_FUNC_SETUP,
            [addresses.len(), 0, 0],
        );

        if printing {
            println!("Setup: {:x?}", result);
        }

        if result.rax != 0x664200006642 {
            println!("Failed to setup");
        }
    }

    apply_patch(&coverage_collector::PATCH); // todo: remove
    interface.reset_coverage();
    disable_all_hooks();

    println!(" ---- NORMAL ---- ");
    print_status();

    println!(" ---- SETUP ---- ");
    let addr = [
        UCInstructionAddress::from_const(0x428),
        UCInstructionAddress::from_const(0x42a),
        UCInstructionAddress::from_const(0x1862),
        UCInstructionAddress::from_const(0x1864),
        UCInstructionAddress::from_const(0x1866),
        UCInstructionAddress::from_const(0x1868),
        UCInstructionAddress::from_const(0x186a),
    ];
    let instructions: [InstructionTableEntry; 7] = [
        [0x01, 0x02, 0x03, 0x04],
        [0x11, 0x12, 0x13, 0x14],
        [0x21, 0x22, 0x23, 0x24],
        [0x31, 0x32, 0x33, 0x34],
        [0x41, 0x42, 0x43, 0x44],
        [0x51, 0x52, 0x53, 0x54],
        [0x61, 0x62, 0x63, 0x64],
    ];

    setup_hooks(&mut interface, &addr, &instructions, true);

    println!(" ---- SETUP ---- ");
    print_status();

    /* for i in (0x7c00..0x7e00).filter(|i| i % 4 != 3) {
        let i = UCInstructionAddress::from_const(i);
        let instruction = ms_patch_instruction_read(
            coverage_collector::LABEL_FUNC_LDAT_READ,
            i,
        );
        if instruction == 0x01 {
            println!("Found instruction at {}", i);
        }
    } */

    println!(" ---- SPEEDTEST ---- ");

    let before = match uefi::runtime::get_time() {
        Ok(time) => time,
        Err(e) => {
            println!("Failed to get time: {:?}", e);
            return Status::ABORTED;
        }
    };
    let count = 1e6 as usize;
    for _ in 0..count {
        core::hint::black_box(setup_hooks)(&mut interface, &addr, &instructions, false);
    }
    let after = match uefi::runtime::get_time() {
        Ok(time) => time,
        Err(e) => {
            println!("Failed to get time: {:?}", e);
            return Status::ABORTED;
        }
    };

    let difference = (after.minute() - before.minute()) as usize * 60e9 as usize + (after.second() - before.second()) as usize * 1e9 as usize + (after.nanosecond() - before.nanosecond()) as usize;

    println!("For {} iterations: {}ns", count, difference);
    println!(" {}ns per iteration", difference as f64 / count as f64);

    println!("Goodbye!");

    // cleanup in reverse order
    cpu.cleanup();

    Status::SUCCESS
}
