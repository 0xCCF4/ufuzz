#![no_main]
#![no_std]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::arch::asm;
use coverage::coverage_harness::{CoverageHarness, ExecutionResultEntry};
use coverage::interface::safe::ComInterface;
use coverage::{coverage_collector, interface_definition};
use custom_processing_unit::{apply_patch, lmfence, CustomProcessingUnit, FunctionResult};
use data_types::addresses::{Address, UCInstructionAddress};
use itertools::Itertools;
use log::info;
use uefi::prelude::*;
use uefi::{print, println, CString16};
use uefi::proto::media::file::{File, FileAttribute, FileMode};

#[entry]
unsafe fn main() -> Status {
    uefi::helpers::init().unwrap();
    println!("Hello world! v2");

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

    apply_patch(&coverage_collector::PATCH);

    interface.reset_coverage();

    let mut harness = CoverageHarness::new(&mut interface, &cpu);

    let blacklisted = match read_blacklisted() {
        Ok(blacklisted) => blacklisted,
        Err(e) => {
            info!("Failed to read blacklisted addresses: {:?}", e);
            return Status::ABORTED;
        }
    };

    let covered = match read_covered() {
        Ok(covered) => covered,
        Err(e) => {
            info!("Failed to read covered addresses: {:?}", e);
            return Status::ABORTED;
        }
    };

    let mut coverage_map = BTreeMap::new();
    for chunk in covered.into_iter() {
        let addresses = UCInstructionAddress::from_const(chunk);

        if let Err(err) = harness.is_hookable(addresses) {
            println!("Not hookable: {:?}", err);
            continue;
        }

        if blacklisted.iter().contains(&chunk) {
            println!("Blacklisted");
            continue;
        }

        let mut result_step = None;
        for i in 0..30 {
            print!("\r[{:04x}] {i:2} ", chunk);
            let result = harness.execute(
                &[addresses],
                |_| {
                    rdrand()
                },
                (),
            );
            print!("{i:2} ");
            let result = match result {
                Err(e) => {
                    println!("Failed to execute harness: {:?}", e);
                    None
                },
                Ok(result) => {
                    if !result.result.0 {
                        println!("Failed to execute harness: {:?}", result.result);
                        return Status::ABORTED;
                    }
                    match result.hooks[0] {
                        ExecutionResultEntry::NotCovered => None,
                        ExecutionResultEntry::Covered {
                            count: _,
                            timing
                        } => Some(timing),
                    }
                }
            };

            match (result, result_step) {
                (None, None) => continue,
                (None, Some(_)) => {
                    println!("Failed to execute harness after successful iteration: {:?}", result_step);
                    return Status::ABORTED;
                }
                (Some(_), None) | (Some(_), Some(_)) => {
                    result_step = result;
                }
            }
        }

        if let Some(step) = result_step {
            coverage_map.insert(UCInstructionAddress::from_const(chunk), step);
        }
    }

    println!("\rResults:");
    coverage_map.iter().sorted_by_key(|(_address, timing)| *timing).for_each(|(address, timing)| {
        println!("{:04x} {}", address.address(), timing);
        if let Err(err) = append_string_to_file(format!("{:04x}: {}\n", address.address(), timing).as_str(), "timing.txt") {
            println!("Failed to append to file: {:?}", err);
        }
    });

    drop(harness);

    if let Err(err) = cpu.zero_hooks() {
        println!("Failed to zero hooks: {:?}", err);
    }

    println!("Goodbye!");

    // cleanup in reverse order
    cpu.cleanup();

    Status::SUCCESS
}

fn file_name(name: &str) -> uefi::Result<CString16> {
    const PREFIX: &str = "";

    CString16::try_from(format!("{}{}", PREFIX, name).as_str())
        .map_err(|_| uefi::Error::from(uefi::Status::UNSUPPORTED))
}

fn append_string_to_file(text: &str, name: &str) -> uefi::Result<()> {
    let mut proto = uefi::boot::get_image_file_system(uefi::boot::image_handle())?;
    let mut root_dir = proto.open_volume()?;
    let mut dir = root_dir.open(
        file_name("test_filter")?.as_ref(),
        FileMode::CreateReadWrite,
        FileAttribute::DIRECTORY
    )?;
    let file = dir.open(
        file_name(name)?.as_ref(),
        FileMode::CreateReadWrite,
        FileAttribute::empty(),
    )?;

    let mut regular_file = file
        .into_regular_file()
        .ok_or_else(|| uefi::Error::from(uefi::Status::UNSUPPORTED))?;

    let mut buffer = [0u8; 128];
    let mut data = String::new();

    loop {
        let read = regular_file.read(&mut buffer)?;

        if read == 0 {
            break;
        }

        for i in 0..read {
            data.push(buffer[i] as char);
        }
    }

    data.push_str(text);

    regular_file.set_position(0)?;

    regular_file
        .write(data.as_bytes())
        .map_err(|_| uefi::Error::from(uefi::Status::WARN_WRITE_FAILURE))?;

    regular_file.flush()?;
    regular_file.close();

    root_dir.flush()?;
    root_dir.close();

    Ok(())
}

fn read_file(name: &str) -> uefi::Result<Vec<(usize, String)>> {
    let mut proto = uefi::boot::get_image_file_system(uefi::boot::image_handle())?;
    let mut root_dir = proto.open_volume()?;
    let mut dir = root_dir.open(
        file_name("test_filter")?.as_ref(),
        FileMode::CreateReadWrite,
        FileAttribute::DIRECTORY
    )?;
    let file = dir.open(
        file_name(name)?.as_ref(),
        FileMode::CreateReadWrite,
        FileAttribute::empty(),
    )?;

    let mut regular_file = file
        .into_regular_file()
        .ok_or_else(|| uefi::Error::from(uefi::Status::UNSUPPORTED))?;

    let mut buffer = [0u8; 128];
    let mut data = String::new();

    loop {
        let read = regular_file.read(&mut buffer)?;

        if read == 0 {
            break;
        }

        for i in 0..read {
            data.push(buffer[i] as char);
        }
    }

    Ok(data
        .lines()
        .filter(|line| {
            !(line.starts_with("//") || line.starts_with("#") || line.is_empty())
        })
        .filter_map(|line| {
            let (address, reason) = line.split_once(" ").unwrap_or((line, ""));
            usize::from_str_radix(address, 16)
                .map_err(|_| uefi::Error::from(uefi::Status::UNSUPPORTED)).ok().map(|address| (address, reason.to_string()))
        })
        .collect())
}

fn read_blacklisted() -> uefi::Result<Vec<usize>> {
    let additional_blacklist = include!("../src/blacklist.gen");

    Ok(read_file("blacklist.txt")?
        .into_iter()
        .map(|(address, _)| address)
        .chain(additional_blacklist.into_iter())
        .sorted()
        .collect_vec())
}

fn read_covered() -> uefi::Result<Vec<usize>> {
    let result = read_file("covered.txt")?
        .into_iter()
        .map(|(address, _)| address)
        .sorted();
    let mut final_result = Vec::new();
    for address in result {
        if !final_result.contains(&address) {
            final_result.push(address);
        }
    }
    Ok(final_result)
}

#[allow(dead_code)]
fn read_skipped() -> uefi::Result<Vec<(usize, String)>> {
    read_file("skipped.txt")
}

fn rdrand() -> (bool, FunctionResult) {
    let mut result = FunctionResult::default();
    let flags: u8;
    lmfence();
    unsafe {
        asm! {
        "xchg {rbx_tmp}, rbx",
        "rdrand rax",
        "setc {flags}",
        "xchg {rbx_tmp}, rbx",
        inout("rax") 0usize => result.rax,
        rbx_tmp = inout(reg) 0usize => result.rbx,
        inout("rcx") 0usize => result.rcx,
        inout("rdx") 0usize => result.rdx,
        flags = out(reg_byte) flags,
        options(nostack),
        }
    }
    lmfence();
    (flags > 0, result)
}
