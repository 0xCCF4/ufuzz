#![no_main]
#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use custom_processing_unit::{apply_patch, call_custom_ucode_function, CustomProcessingUnit};
use itertools::Itertools;
use log::info;
use uefi::prelude::*;
use uefi::{print, println, CString16};
use uefi::proto::media::file::{File, FileAttribute, FileMode};

mod patch {
    use ucode_compiler_derive::patch;

    patch!(
        .org 0x7c00

        <entry>
        rax := LDSTGBUF_DSZ64_ASZ16_SC1(tmp0) !m2
        rbx := ZEROEXT_DSZ64(0x4822)
        unk_256() !m1 SEQW LFNCEWAIT, UEND0

        NOP
    );
}

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

    apply_patch(&patch::PATCH);

    let last_ok = match read_ok() {
        Ok(last_ok) => last_ok,
        Err(e) => {
            info!("Failed to read last ok address: {:?}", e);
            return Status::ABORTED;
        }
    };

    let mut next_address = 0;
    if last_ok > 0 {
        println!("Last ok address: {:04x}", last_ok);
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
        println!(" - {:04x}", address);
    }

    println!("Next address to test: {:04x}", next_address);

    if next_address >= 0x10000 {
        info!("No more addresses to test");
        return Status::SUCCESS;
    }

    for chunk in (next_address..0x10000).into_iter() {
        let _ = uefi::boot::set_watchdog_timer(20, 0x10000, None);
        print!("\r                                \r[{:04x}] ", chunk);

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

        let result = call_custom_ucode_function(patch::LABEL_ENTRY, [chunk, 0, 0]);
        if result.rbx != 0x4822 {
            println!("Failed: {:x?}", result);
            return Status::ABORTED;
        }
        print!("{:016x?}", result.rax);

        if let Err(e) = write_ok(chunk) {
            println!("Failed to write ok: {:?}", e);
            return Status::ABORTED;
        }
        if let Err(e) = write_ok(chunk) {
            println!("Failed to write ok: {:?}", e);
            return Status::ABORTED;
        }
    }

    let _ = uefi::boot::set_watchdog_timer(60, 0x10000, None);
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

fn read_ok() -> uefi::Result<usize> {
    let mut proto = uefi::boot::get_image_file_system(uefi::boot::image_handle())?;
    let mut root_dir = proto.open_volume()?;
    let mut dir = root_dir.open(
        file_name("test_staging_buffer")?.as_ref(),
        FileMode::CreateReadWrite,
        FileAttribute::DIRECTORY
    )?;
    let file = dir.open(
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
    let mut dir = root_dir.open(
        file_name("test_staging_buffer")?.as_ref(),
        FileMode::CreateReadWrite,
        FileAttribute::DIRECTORY
    )?;
    let file = dir.open(
        file_name("ok.txt")?.as_ref(),
        FileMode::CreateReadWrite,
        FileAttribute::empty(),
    )?;

    let mut regular_file = file
        .into_regular_file()
        .ok_or_else(|| uefi::Error::from(uefi::Status::UNSUPPORTED))?;

    regular_file
        .write(format!("{:04x}\n", address).as_bytes())
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
    let mut dir = root_dir.open(
        file_name("test_staging_buffer")?.as_ref(),
        FileMode::CreateReadWrite,
        FileAttribute::DIRECTORY
    )?;
    let file = dir.open(
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

    data.push_str(format!("{:04x}\n", new_address).as_str());

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
    let mut dir = root_dir.open(
        file_name("test_staging_buffer")?.as_ref(),
        FileMode::CreateReadWrite,
        FileAttribute::DIRECTORY
    )?;
    let file = dir.open(
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
