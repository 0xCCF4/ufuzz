#![no_main]
#![no_std]

extern crate alloc;

use coverage::harness::coverage_harness::CoverageHarness;
use coverage::interface::safe::ComInterface;
use coverage::interface_definition;
use custom_processing_unit::CustomProcessingUnit;
use data_types::addresses::UCInstructionAddress;
use itertools::Itertools;
use log::info;
use uefi::prelude::*;
use uefi::{print, println};

#[entry]
unsafe fn main() -> Status {
    uefi::helpers::init().unwrap();
    info!("Hello world!");

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

    let harness = CoverageHarness::new(&mut interface, &cpu);

    for chunk in (0..0x7c00)
        .filter(|i| (i % 2) == 0 && (i % 4) < 3)
        .chunks(hooks as usize)
        .into_iter()
    {
        let addresses = chunk
            .map(|i| UCInstructionAddress::from_const(i))
            .collect_vec();

        if addresses.is_empty() {
            break;
        }

        print!(
            "\r[{}]->[{}]: ",
            &addresses.first().unwrap(),
            &addresses.last().unwrap()
        );

        /* if let Err(e) = harness.execute(
            &addresses,
            |_| {
                for _ in 0..32 {
                    rdrand();
                }
            },
            (),
        ) {
            println!("Failed to execute harness: {:?}", e);
            return Status::ABORTED;
        }

        if addresses.iter().any(|a| harness.covered(a)) {
            print!("Covered: ");
            for address in &addresses {
                if harness.covered(address) {
                    print!("{}, ", address);
                }
            }
            println!();
        }*/
    }

    drop(harness);

    if let Err(err) = cpu.zero_hooks() {
        println!("Failed to zero hooks: {:?}", err);
    }

    println!("Goodbye!");
    Status::SUCCESS
}
