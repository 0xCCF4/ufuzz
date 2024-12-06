#![no_main]
#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::arch::asm;
use coverage::coverage_harness::CoverageHarness;
use coverage::interface::safe::ComInterface;
use coverage::{coverage_collector, interface_definition};
use custom_processing_unit::{apply_patch, lmfence, CustomProcessingUnit, FunctionResult};
use data_types::addresses::UCInstructionAddress;
use itertools::Itertools;
use log::info;
use uefi::prelude::*;
use uefi::proto::media::file::{File, FileAttribute, FileMode};
use uefi::{print, println, CString16};

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

    apply_patch(&coverage_collector::PATCH);

    interface.reset_coverage();

    let mut harness = CoverageHarness::new(&mut interface, &cpu);
    harness.init();

    let last_ok = match read_ok() {
        Ok(last_ok) => last_ok,
        Err(e) => {
            info!("Failed to read last ok address: {:?}", e);
            return Status::ABORTED;
        }
    };

    let mut next_address = 0;
    if last_ok > 0 {
        println!("Last ok address: {:05x}", last_ok);
        if let Err(e) = write_blacklisted(last_ok + 1) {
            info!("Failed to write blacklisted address: {:?}", e);
            return Status::ABORTED;
        }
        next_address = last_ok + 1;
    }

    let blacklisted = match read_blacklisted() {
        Ok(blacklisted) => blacklisted,
        Err(e) => {
            info!("Failed to read blacklisted addresses: {:?}", e);
            return Status::ABORTED;
        }
    };

    println!("Blacklisted addresses:");
    for address in blacklisted.iter() {
        println!(" - {:05x}", address);
    }

    println!("Next address to test: {:05x}", next_address);

    if next_address >= 0x7c00 {
        info!("No more addresses to test");
        return Status::SUCCESS;
    }

    for chunk in (next_address..0x7c00).into_iter() {
        let addresses = UCInstructionAddress::from_const(chunk);

        print!("\r[{}] ", &addresses);

        if (chunk % 2) > 0 || (chunk % 4) >= 3 {
            // skip
            if let Err(e) = write_ok(chunk) {
                println!("Failed to write ok: {:?}", e);
                return Status::ABORTED;
            }
            if let Err(e) = write_ok(chunk) {
                println!("Failed to write ok: {:?}", e);
                return Status::ABORTED;
            }
            continue;
        }

        if blacklisted.iter().contains(&chunk) {
            println!("Blacklisted");
            if let Err(e) = write_ok(chunk) {
                println!("Failed to write ok: {:?}", e);
                return Status::ABORTED;
            }
            if let Err(e) = write_ok(chunk) {
                println!("Failed to write ok: {:?}", e);
                return Status::ABORTED;
            }
            continue;
        }

        if let Err(e) = harness.execute(
            &[addresses],
            |_| {
                rdrand();
            },
            (),
        ) {
            println!("Failed to execute harness: {:?}", e);
            return Status::ABORTED;
        }

        if let Err(e) = write_ok(chunk) {
            println!("Failed to write ok: {:?}", e);
            return Status::ABORTED;
        }
        if let Err(e) = write_ok(chunk) {
            println!("Failed to write ok: {:?}", e);
            return Status::ABORTED;
        }

        if harness.covered(addresses) {
            println!("Covered");
        }
    }

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
    const PREFIX: &str = "test_filter_";

    CString16::try_from(format!("{}{}", PREFIX, name).as_str())
        .map_err(|_| uefi::Error::from(uefi::Status::UNSUPPORTED))
}

fn read_ok() -> uefi::Result<usize> {
    let mut proto = uefi::boot::get_image_file_system(uefi::boot::image_handle())?;
    let mut root_dir = proto.open_volume()?;
    let file = root_dir.open(
        file_name("ok.txt")?.as_ref(),
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
            return Ok(0);
        }

        for i in 0..read {
            if buffer[i] == '\n' as u8 {
                return Ok(usize::from_str_radix(&data, 16)
                    .map_err(|_| uefi::Error::from(uefi::Status::UNSUPPORTED))?);
            }
            data.push(buffer[i] as char);
        }
    }
}

fn write_ok(address: usize) -> uefi::Result<()> {
    let mut proto = uefi::boot::get_image_file_system(uefi::boot::image_handle())?;
    let mut root_dir = proto.open_volume()?;
    let file = root_dir.open(
        file_name("ok.txt")?.as_ref(),
        FileMode::CreateReadWrite,
        FileAttribute::empty(),
    )?;

    let mut regular_file = file
        .into_regular_file()
        .ok_or_else(|| uefi::Error::from(uefi::Status::UNSUPPORTED))?;

    regular_file
        .write(format!("{:05x}\n", address).as_bytes())
        .map_err(|_| uefi::Error::from(uefi::Status::WARN_WRITE_FAILURE))?;

    regular_file.flush()?;
    regular_file.close();

    root_dir.flush()?;
    root_dir.close();

    drop(proto);

    uefi::boot::stall(100);

    Ok(())
}

fn write_blacklisted(new_address: usize) -> uefi::Result<()> {
    if new_address >= 0x7c00 {
        return Ok(());
    }

    let mut proto = uefi::boot::get_image_file_system(uefi::boot::image_handle())?;
    let mut root_dir = proto.open_volume()?;
    let file = root_dir.open(
        file_name("blacklist.txt")?.as_ref(),
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

    data.push_str(format!("{:05x}\n", new_address).as_str());

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

fn read_blacklisted() -> uefi::Result<Vec<usize>> {
    let mut proto = uefi::boot::get_image_file_system(uefi::boot::image_handle())?;
    let mut root_dir = proto.open_volume()?;
    let file = root_dir.open(
        file_name("blacklist.txt")?.as_ref(),
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
        .map(|line| {
            usize::from_str_radix(line, 16)
                .map_err(|_| uefi::Error::from(uefi::Status::UNSUPPORTED))
        })
        .filter_map(|v| v.ok())
        .collect())
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
