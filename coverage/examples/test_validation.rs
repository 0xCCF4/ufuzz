#![no_main]
#![no_std]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::arch::asm;
use core::ops::{AddAssign, SubAssign};
use coverage::harness::coverage_harness::hookable_address_iterator::HookableAddressIterator;
use coverage::harness::coverage_harness::modification_engine::ModificationEngineSettings;
use coverage::harness::coverage_harness::CoverageHarness;
use coverage::harness::iteration_harness::IterationHarness;
use coverage::interface::safe::ComInterface;
use coverage::interface_definition;
use coverage::interface_definition::CoverageCount;
use custom_processing_unit::{lmfence, CpuidResult, CustomProcessingUnit, FunctionResult};
use data_types::addresses::{Address, UCInstructionAddress};
use itertools::Itertools;
use log::info;
use uefi::prelude::*;
use uefi::proto::media::file::{File, FileAttribute, FileMode};
use uefi::{print, println, CString16};

#[entry]
unsafe fn main() -> Status {
    uefi::helpers::init().unwrap();
    println!("Hello world!");

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

    let itd = &interface_definition::COM_INTERFACE_DESCRIPTION;

    let mut interface = match ComInterface::new(itd) {
        Ok(interface) => interface,
        Err(e) => {
            info!("Failed to initiate program {:?}", e);
            return Status::ABORTED;
        }
    };

    let hookable_addresses = HookableAddressIterator::construct(
        cpu.rom(),
        &ModificationEngineSettings::default(),
        itd.max_number_of_hooks.min(1),
    );

    println!("Hookable addresses: {:04x}", hookable_addresses.len() * 2);

    let mut coverage_harness = CoverageHarness::new(&mut interface, &cpu);
    //let validation_harness = ValidationHarness::new(&mut coverage_harness);
    let iteration_harness = IterationHarness::new(hookable_addresses);

    let duplicate_found = iteration_harness
        .execute(|chunk| {
            for addr in chunk {
                if chunk
                    .iter()
                    .filter_map(|a| {
                        if addr.triad_base() == a.triad_base() {
                            Some(a.triad_base())
                        } else {
                            None
                        }
                    })
                    .count()
                    > 1
                {
                    println!("Duplicated triad base: {:?}", addr);
                    return true;
                }
            }
            false
        })
        .fold(false, |a, b| a || b);

    if duplicate_found {
        return Status::ABORTED;
    }

    println!(" == NO FUNCTION == ");
    let baseline = collect_coverage(&iteration_harness, &mut coverage_harness, None, || {
        // do nothing
    });

    println!(" == RDRAND == ");
    collect_coverage(
        &iteration_harness,
        &mut coverage_harness,
        Some(&baseline),
        || {
            rdrand();
        },
    );

    println!(" == 2x RDRAND == ");
    collect_coverage(
        &iteration_harness,
        &mut coverage_harness,
        Some(&baseline),
        || {
            rdrand();
            rdrand();
        },
    );

    println!(" == CPUID == ");
    let expected = CpuidResult::query(0x1, 0);
    collect_coverage(
        &iteration_harness,
        &mut coverage_harness,
        Some(&baseline),
        || {
            let result = CpuidResult::query(0x1, 0);
            if result != expected {
                println!("Unexpected result: {:?} != {:?}", result, expected);
            }
        },
    );

    println!(" == CPUID FULL == ");
    test_cpuid(&iteration_harness, &mut coverage_harness, Some(&baseline));

    println!("Goodbye!");

    Status::SUCCESS
}

fn collect_coverage<R, F: FnMut() -> R>(
    iteration_harness: &IterationHarness,
    coverage_harness: &mut CoverageHarness,
    baseline: Option<&BTreeMap<UCInstructionAddress, CoverageCount>>,
    mut func: F,
) -> BTreeMap<UCInstructionAddress, CoverageCount> {
    let results = iteration_harness.execute(|chunk| coverage_harness.execute(chunk, || func()));

    let mut coverage = BTreeMap::new();
    for result in results {
        match result {
            Ok(value) => {
                for hook in value.hooks {
                    if hook.covered() {
                        coverage.insert(hook.address(), hook.coverage());
                    }
                }
            }
            Err(err) => {
                println!("Error {:?} ", err)
            }
        }
    }

    remove_baseline_from_coverage(baseline, &mut coverage);

    print_coverage(&coverage);

    coverage
}

fn print_coverage(coverage: &BTreeMap<UCInstructionAddress, CoverageCount>) {
    for chunk in coverage
        .iter()
        .sorted_by_key(|(key, _)| **key)
        .collect_vec()
        .chunks(12)
    {
        for (address, value) in chunk {
            print!("{address}:{} ", value)
        }
        println!()
    }
}

fn remove_baseline_from_coverage(
    baseline: Option<&BTreeMap<UCInstructionAddress, CoverageCount>>,
    coverage: &mut BTreeMap<UCInstructionAddress, CoverageCount>,
) {
    if let Some(baseline) = baseline {
        for (address, value) in coverage.iter_mut() {
            if let Some(baseline_value) = baseline.get(address) {
                if baseline_value > value {
                    println!(
                        "Coverage decreased below zero with baseline! {}: {:?} -> {:?}",
                        address, baseline_value, value
                    );
                    *value = 0;
                } else {
                    value.sub_assign(baseline_value);
                }
            }
        }

        coverage.retain(|_, value| *value > 0);
    }
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

fn test_cpuid(
    iteration_harness: &IterationHarness,
    coverage_harness: &mut CoverageHarness,
    baseline: Option<&BTreeMap<UCInstructionAddress, CoverageCount>>,
) -> BTreeMap<UCInstructionAddress, u16> {
    print!(" Collecting samples...");
    let pass1 = collect_cpuids_all::<&Vec<u32>>(None);
    let pass2 = collect_cpuids_all::<&Vec<u32>>(None);
    println!("OK");

    let mut stable = Vec::with_capacity(pass1.len());
    for (a, b) in pass1.iter().zip(pass2.iter()) {
        if a.1 != b.1 {
            println!(
                "CPUID mismatch WITHOUT hooks present, at index {}: {:?} != {:?}",
                a.0, a.1, b.1
            );
        } else {
            stable.push(a.0);
        }
    }

    let no_coverage_comparison_value = collect_cpuids_all(Some(&stable));

    let mut coverage_map = BTreeMap::new();

    let blacklisted = read_blacklisted().unwrap_or_default();

    println!("Collecting coverage...");
    for result in iteration_harness.execute(|chunk| {
        if blacklisted
            .iter()
            .contains(&chunk[0].align_even().address())
            || blacklisted
                .iter()
                .contains(&(chunk[0].align_even().address() + 1))
        {
            None
        } else {
            Some(coverage_harness.execute(chunk, || {
                print!("\r{:04x?}", chunk);
                collect_cpuids_all(Some(&stable))
            }))
        }
    }) {
        match result {
            Some(Ok(value)) => {
                if value.result != no_coverage_comparison_value {
                    println!("CPUID mismatch WITH hooks present: {:?}", value);
                }

                for hook in value.hooks {
                    if hook.covered() {
                        let address = hook.address();
                        let coverage = hook.coverage();
                        coverage_map
                            .entry(address)
                            .or_insert(0)
                            .add_assign(coverage);
                    }
                }
            }
            Some(Err(err)) => {
                println!("Error {:?} ", err)
            }
            None => {}
        }
    }

    remove_baseline_from_coverage(baseline, &mut coverage_map);
    print_coverage(&coverage_map);

    coverage_map
}

fn collect_cpuids_all<'a, I: IntoIterator<Item = &'a u32>>(
    keys: Option<I>,
) -> Vec<(u32, CpuidResult)> {
    let mut result = Vec::new();

    match keys {
        Some(keys) => {
            for key in keys {
                result.push((*key, CpuidResult::query(*key, 0)));
            }
        }
        None => {
            for key in (0..0x20)
                .into_iter()
                .chain((0x80000000..80000008).into_iter())
            {
                result.push((key, CpuidResult::query(key, 0)));
            }
        }
    }

    result
}

fn file_name(name: &str) -> uefi::Result<CString16> {
    const PREFIX: &str = "";

    CString16::try_from(format!("{}{}", PREFIX, name).as_str())
        .map_err(|_| uefi::Error::from(uefi::Status::UNSUPPORTED))
}

fn read_file(name: &str) -> uefi::Result<Vec<usize>> {
    let mut proto = uefi::boot::get_image_file_system(uefi::boot::image_handle())?;
    let mut root_dir = proto.open_volume()?;
    let mut dir = root_dir.open(
        file_name("test_filter")?.as_ref(),
        FileMode::Read,
        FileAttribute::DIRECTORY,
    )?;
    let file = dir.open(
        file_name(name)?.as_ref(),
        FileMode::Read,
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
        .filter(|line| !(line.starts_with("//") || line.starts_with("#") || line.is_empty()))
        .filter_map(|line| {
            usize::from_str_radix(line, 16)
                .map_err(|_| uefi::Error::from(uefi::Status::UNSUPPORTED))
                .ok()
                .map(|address| address)
        })
        .collect())
}

fn read_blacklisted() -> uefi::Result<Vec<usize>> {
    Ok(read_file("blacklist.txt")?
        .into_iter()
        .sorted()
        .collect_vec())
}
