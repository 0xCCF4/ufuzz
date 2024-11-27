#![no_main]
#![no_std]

extern crate alloc;

use alloc::format;
use custom_processing_unit::{
    apply_patch,
    CustomProcessingUnit,
};
use log::info;
use uefi::prelude::*;
use uefi::{print, println, CStr16};
use uefi::proto::media::file::{File, FileAttribute, FileMode};
use coverage::udump::dump_ucode;
use data_types::addresses::UCInstructionAddress;

mod dump_patch {
    use ucode_compiler_derive::patch;

    patch!(
        .org 0x7c00

        <func_dump_memory>
        # IN: tmp0 - address to dump to
        func sys/func_dump_memory(tmp0, tmp10, tmp11, tmp12)

        rax := ZEROEXT_DSZ32(0x4421)
        rax := CONCAT_DSZ32(rax, 0x4421)
    );
}

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

    apply_patch(&dump_patch::PATCH);

    let mut fs = match uefi::boot::get_image_file_system(uefi::boot::image_handle()) {
        Ok(fs) => fs,
        Err(e) => {
            info!("Failed to get image file system {:?}", e);
            return Status::ABORTED;
        }
    };

    let mut root = match fs.open_volume() {
        Ok(root) => root,
        Err(e) => {
            info!("Failed to open volume {:?}", e);
            return Status::ABORTED;
        }
    };

    const NAME_C: &str = "dump.txt";
    let mut buf = [0u16; NAME_C.len() + 4];
    let name = match CStr16::from_str_with_buf(NAME_C, &mut buf) {
        Ok(name) => name,
        Err(e) => {
            info!("Failed to convert name {:?}", e);
            return Status::ABORTED;
        }
    };

    let file = match root.open(name, FileMode::CreateReadWrite, FileAttribute::empty()) {
        Ok(file) => file,
        Err(e) => {
            info!("Failed to open file {:?}", e);
            return Status::ABORTED;
        }
    };

    let mut file = match file.into_regular_file() {
        Some(file) => file,
        None => {
            info!("Failed to convert file to regular file");
            return Status::ABORTED;
        }
    };

    let dump = match dump_ucode(dump_patch::LABEL_FUNC_DUMP_MEMORY) {
        Ok(dump) => dump,
        Err(e) => {
            info!("Failed to dump ucode {:?}", e);
            return Status::ABORTED;
        }
    };

    if let Err(e) = file.write("Address | ins[0] | ins[1] | ins[2] | SEQW".as_bytes()) {
        info!("Failed to write to file {:?}", e);
        return Status::ABORTED;
    }
    for (i, row) in dump.iter().enumerate() {
        let mut text = format!("[{}]", UCInstructionAddress::ZERO + i*4);

        for x in row.iter() {
            text.push_str(format!(" {:012x}", x).as_str());
        }

        text.push_str("\n");

        print!("{}", text.as_str());
        if let Err(e) = file.write(text.as_bytes()) {
            info!("Failed to write to file {:?}", e);
            return Status::ABORTED;
        }
    }

    println!("Goodbye!");

    cpu.cleanup();

    Status::SUCCESS
}
