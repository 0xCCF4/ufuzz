#![no_main]
#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::arch::asm;
use core::fmt::Debug;
use coverage::coverage_harness::{CoverageError, CoverageHarness};
use coverage::interface::safe::ComInterface;
use coverage::{coverage_collector, interface_definition};
use custom_processing_unit::{apply_patch, lmfence, CustomProcessingUnit, FunctionResult};
use data_types::addresses::{UCInstructionAddress};
use itertools::Itertools;
use log::info;
use uefi::prelude::*;
use uefi::{print, println, CString16};
use uefi::proto::media::file::{File, FileAttribute, FileMode};
use uefi::runtime::ResetType;

const WRITE_PROTECT: bool = true;

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
        if !WRITE_PROTECT {
            if let Err(e) = write_blacklisted(last_ok + 1) {
                info!("Failed to write blacklisted address: {:?}", e);
                return Status::ABORTED;
            }
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

    println!("Blacklisted addresses: {}", blacklisted.len());

    println!("Next address to test: {:04x}", next_address);

    if WRITE_PROTECT {
        next_address = 0;
    }

    let mut skipped = 0;
    let mut excluded = 0;
    let mut total = 0;
    let mut worked = 0;

    if next_address >= 0x7c00 {
        println!("No more addresses to test");
    } else {
        for chunk in (next_address..0x7c00).into_iter() {
            let addresses = UCInstructionAddress::from_const(chunk);

            if (chunk % 2) > 0 || (chunk % 4) >= 3 {
                // skip
                if !WRITE_PROTECT {
                    if let Err(e) = write_ok(chunk) {
                        println!("Failed to write ok: {:?}", e);
                        return Status::ABORTED;
                    }
                }
                continue;
            }

            total += 1;

            print!("\r[{}] ", &addresses);

            if let Err(err) = harness.is_hookable(addresses) {
                println!("Not hookable: {err:?}");
                skipped += 1;
                if !WRITE_PROTECT {
                    if let Err(e) = write_ok(chunk) {
                        println!("Failed to write ok: {:?}", e);
                        return Status::ABORTED;
                    }
                }
                continue;
            }

            if blacklisted.iter().contains(&chunk) {
                println!("Blacklisted");
                excluded += 1;
                if !WRITE_PROTECT {
                    if let Err(e) = write_ok(chunk) {
                        println!("Failed to write ok: {:?}", e);
                        return Status::ABORTED;
                    }
                }
                continue;
            }

            match harness.execute(
                &[addresses],
                |_| {
                    rdrand()
                },
                (),
            ) {
                Err(e) => {
                    //println!("Failed to execute harness: {:?}", e);
                    match e {
                        CoverageError::SetupFailed(_) => {
                            if let Err(err) = write_skipped(chunk, &e) {
                                println!("Failed to write skipped: {:?}", err);
                                return Status::ABORTED;
                            }
                            uefi::runtime::reset(ResetType::COLD, Status::SUCCESS, None)
                        },
                        _ => continue
                    }
                }
                Ok(r) => {
                    if r.hooks[0].covered() {
                        println!("Covered");
                        if !WRITE_PROTECT {
                            if let Err(e) = write_covered(chunk) {
                                println!("Failed to write covered: {:?}", e);
                                return Status::ABORTED;
                            }
                        }
                    }
                }
            }

            worked += 1;

            if !WRITE_PROTECT {
                if let Err(e) = write_ok(chunk) {
                    println!("Failed to write ok: {:?}", e);
                    return Status::ABORTED;
                }
                if let Err(e) = write_actually_works(chunk) {
                    println!("Failed to write actually works: {:?}", e);
                    return Status::ABORTED;
                }
            }
        }
    }

    let mut summary = String::new();

    if !WRITE_PROTECT {
        let actually_worked = match read_actually_works() {
            Ok(actually_worked) => actually_worked,
            Err(e) => {
                println!("Failed to read actually worked addresses: {:?}", e);
                return Status::ABORTED;
            }
        };
        let blacklisted = match read_blacklisted() {
            Ok(blacklisted) => blacklisted,
            Err(e) => {
                println!("Failed to read blacklisted addresses: {:?}", e);
                return Status::ABORTED;
            }
        };
        let blacklisted_raw = match read_blacklisted_raw() {
            Ok(blacklisted_raw) => blacklisted_raw,
            Err(e) => {
                println!("Failed to read blacklisted raw addresses: {:?}", e);
                return Status::ABORTED;
            }
        };
        let covered = match read_covered() {
            Ok(covered) => covered,
            Err(e) => {
                println!("Failed to read covered addresses: {:?}", e);
                return Status::ABORTED;
            }
        };

        let skipped = 0x7c00 - actually_worked.len() - blacklisted_raw.len();

        summary.push_str(format!("Summary\n----------------------------------\n").as_str());
        summary.push_str(format!("Total:           {:5}\n", 0x7c00/2).as_str());
        summary.push_str(format!("Excluded:        {:5} {:5.02}%\n", blacklisted.len(), (blacklisted.len() as f64 / 0x7c00 as f64) * 100.0).as_str());
        summary.push_str(format!("Skipped:         {:5} {:5.02}%\n", skipped, (skipped as f64 / 0x7c00 as f64) * 100.0).as_str());
        summary.push_str(format!("Actually tested: {:5} {:5.02}%\n", actually_worked.len() + blacklisted_raw.len(), ((actually_worked.len() + blacklisted_raw.len()) as f64 / 0x7c00 as f64) * 100.0).as_str());
        summary.push_str(format!(" - Worked:       {:5} {:5.02}%\n", actually_worked.len(), (actually_worked.len() as f64 / 0x7c00 as f64) * 100.0).as_str());
        summary.push_str(format!(" - Blacklisted:  {:5} {:5.02}%\n", blacklisted_raw.len(), (blacklisted_raw.len() as f64 / 0x7c00 as f64) * 100.0).as_str());
        summary.push_str(format!("Covered:         {:5} {:5.02}%\n", covered.len(), (covered.len() as f64 / 0x7c00 as f64) * 100.0).as_str());
        summary.push_str("\n\n\n\n");
    } else {
        summary.push_str(format!("Summary\n----------------------------------\n").as_str());
        summary.push_str(format!("Total:    {:5}\n", total).as_str());
        summary.push_str(format!("Excluded: {:5} {:5.02}%\n", excluded, (excluded as f64 / total as f64) * 100.0).as_str());
        summary.push_str(format!("Skipped:  {:5} {:5.02}%\n", skipped, (skipped as f64 / total as f64) * 100.0).as_str());
        summary.push_str(format!("Worked:   {:5} {:5.02}%\n", worked, (worked as f64 / total as f64) * 100.0).as_str());
        summary.push_str("\n");
    }

    println!("{}", summary);
    if !WRITE_PROTECT {
        if let Err(err) = append_string_to_file(summary.as_str(), "summary.txt") {
            println!("Failed to write summary: {:?}", err);
        }
    }

    println!("Speedtest");

    let before = match uefi::runtime::get_time() {
        Ok(time) => time,
        Err(e) => {
            println!("Failed to get time: {:?}", e);
            return Status::ABORTED;
        }
    };
    let setups = core::hint::black_box(|| {
        let mut setups = 0usize;
        for address in 0..0x7c00 {
            if address % 0x100 == 0 {
                print!("\r{:04x}", address);
            }
            let r = harness.execute(
                &[address.into()],
                |_| {
                    rdrand()
                },
                (),
            );
            if matches!(r, Ok(_)) {
                setups += 1;
            }
        }
        println!();
        setups
    })();
    let after = match uefi::runtime::get_time() {
        Ok(time) => time,
        Err(e) => {
            println!("Failed to get time: {:?}", e);
            return Status::ABORTED;
        }
    };

    let difference = (after.minute() - before.minute()) as usize * 60 as usize
        + (after.second() - before.second()) as usize * 1 as usize;
    let diff_per_setup = difference / setups;

    println!("{}min {}s total for {} iterations", difference/60, difference%60, setups);
    println!("{}s / {}ns per iteration", diff_per_setup, after.nanosecond() - before.nanosecond());

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

fn read_ok() -> uefi::Result<usize> {
    let mut proto = uefi::boot::get_image_file_system(uefi::boot::image_handle())?;
    let mut root_dir = proto.open_volume()?;
    let mut dir = root_dir.open(
        file_name("test_filter")?.as_ref(),
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
        file_name("test_filter")?.as_ref(),
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

fn append_address_to_file(new_address: usize, name: &str, reason: &str) -> uefi::Result<()> {
    append_string_to_file(format!("{:04x} {}\n", new_address, reason).as_str(), name)
}

fn write_blacklisted(new_address: usize) -> uefi::Result<()> {
    append_address_to_file(new_address, "blacklist.txt", "")
}

fn write_actually_works(new_address: usize) -> uefi::Result<()> {
    append_address_to_file(new_address, "actually_works.txt", "")
}

fn write_skipped<T: Debug>(new_address: usize, reason: T) -> uefi::Result<()> {
    append_address_to_file(new_address, "skipped.txt", &format!("{:?}", reason))
}

fn write_covered(new_address: usize) -> uefi::Result<()> {
    append_address_to_file(new_address, "covered.txt", "")
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

fn read_blacklisted_raw() -> uefi::Result<Vec<usize>> {
    Ok(read_file("blacklist.txt")?
        .into_iter()
        .map(|(address, _)| address)
        .sorted()
        .collect_vec())
}

#[allow(dead_code)]
fn read_covered() -> uefi::Result<Vec<usize>> {
    Ok(read_file("covered.txt")?
        .into_iter()
        .map(|(address, _)| address)
        .sorted()
        .collect_vec())
}

#[allow(dead_code)]
fn read_skipped() -> uefi::Result<Vec<(usize, String)>> {
    read_file("skipped.txt")
}

fn read_actually_works() -> uefi::Result<Vec<usize>> {
    Ok(read_file("actually_works.txt")?
        .into_iter()
        .map(|(address, _)| address)
        .sorted()
        .collect_vec())
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
