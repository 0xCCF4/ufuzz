#![no_main]
#![no_std]

//! Runtime test for the coverage collector patch (setup) ucode function.

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use custom_processing_unit::{
    apply_ldat_read_func, call_custom_ucode_function, CustomProcessingUnit,
};
use log::info;
use uefi::prelude::*;
use uefi::proto::media::file::{File, FileAttribute, FileMode};
use uefi::{print, println, CString16};

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

    let lengths = [0x8000, 0x8000, 0x80, 0x20, 0x200];

    for (i, len) in lengths.iter().enumerate() {
        println!(" --- DUMPING MSRAM ARRAY {} ---", i);
        let values = match dump_msram(*len, i as u8) {
            Ok(values) => values,
            Err(e) => {
                println!("Failed to dump msram: {:?}", e);
                return Status::ABORTED;
            }
        };
        let hex = hex_format(values);
        hex.lines().take(4).for_each(|line| println!("{}", line));
        if let Err(err) = write_file(i as u8, hex.as_str()) {
            println!("Failed to write file: {:?}", err);
            return Status::ABORTED;
        }
    }

    println!("Goodbye!");

    // cleanup in reverse order
    cpu.cleanup();

    Status::SUCCESS
}

fn file_name(name: u8) -> uefi::Result<CString16> {
    const PREFIX: &str = "dump_msram";

    CString16::try_from(format!("{}{}.txt", PREFIX, name).as_str())
        .map_err(|_| uefi::Error::from(uefi::Status::UNSUPPORTED))
}

fn write_file(name: u8, content: &str) -> uefi::Result<()> {
    let mut proto = uefi::boot::get_image_file_system(uefi::boot::image_handle())?;
    let mut root_dir = proto.open_volume()?;
    let file = root_dir.open(
        file_name(name)?.as_ref(),
        FileMode::CreateReadWrite,
        FileAttribute::empty(),
    )?;
    file.delete()?;
    let file = root_dir.open(
        file_name(name)?.as_ref(),
        FileMode::CreateReadWrite,
        FileAttribute::empty(),
    )?;

    let mut regular_file = file
        .into_regular_file()
        .ok_or_else(|| uefi::Error::from(uefi::Status::UNSUPPORTED))?;

    let _ = regular_file.write(content.as_bytes());

    regular_file.flush()?;
    regular_file.close();

    root_dir.flush()?;
    root_dir.close();

    drop(proto);

    Ok(())
}

fn hex_format(values: Vec<u64>) -> String {
    let mut result = String::new();
    for (num, chunk) in values.chunks(4).enumerate() {
        result.push_str(format!("{:04x}:  ", num * 4).as_str());
        for (i, value) in chunk.iter().enumerate() {
            result.push_str(format!("{:014x}{}", value, if i < 3 { " " } else { "" }).as_str());
        }
        result.push_str("\n");
    }
    result
}

fn dump_msram(count: usize, array: u8) -> Result<Vec<u64>, String> {
    let mut result = Vec::new();
    let func = apply_ldat_read_func();
    for i in 0..count {
        if (i % 0x10) == 0 || i + 1 == count {
            print!("\rDumping MSRAM array {:04x}...", i);
        }

        let dword_idx = 0;
        let array_sel = array as usize;
        let bank_sel = 0;

        let array_bank_sel =
            0x10000 | ((dword_idx & 0xf) << 12) | ((array_sel & 0xf) << 8) | (bank_sel & 0xf);
        let array_addr = 0xC00000 | (i & 0xffff);

        result
            .push(call_custom_ucode_function(func, [0x6a0, array_bank_sel, array_addr]).rax as u64);
    }
    print!("\r                                \r");
    Ok(result)
}
