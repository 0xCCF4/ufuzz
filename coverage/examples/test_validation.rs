#![no_main]
#![no_std]

extern crate alloc;

use alloc::collections::BTreeMap;
use core::arch::asm;
use core::ops::SubAssign;
use coverage::harness::coverage_harness::hookable_address_iterator::HookableAddressIterator;
use coverage::harness::coverage_harness::modification_engine::ModificationEngineSettings;
use coverage::harness::coverage_harness::CoverageHarness;
use coverage::harness::iteration_harness::IterationHarness;
use coverage::interface::safe::ComInterface;
use coverage::interface_definition;
use coverage::interface_definition::CoverageEntry;
use custom_processing_unit::{lmfence, CpuidResult, CustomProcessingUnit, FunctionResult};
use data_types::addresses::UCInstructionAddress;
use itertools::Itertools;
use log::info;
use uefi::prelude::*;
use uefi::{print, println};

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
        &ModificationEngineSettings::empty(),
        itd.max_number_of_hooks,
    );

    println!("Hookable addresses: {:04x}", hookable_addresses.len() * 2);

    let mut coverage_harness = CoverageHarness::new(&mut interface, &cpu);
    //let validation_harness = ValidationHarness::new(&mut coverage_harness);
    let iteration_harness = IterationHarness::new(hookable_addresses);

    let duplicate_found = iteration_harness
        .execute(
            |chunk| {
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
            },
            itd.max_number_of_hooks as usize,
        )
        .into_iter()
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

    println!("Goodbye!");

    Status::SUCCESS
}

fn collect_coverage<R, F: FnMut() -> R>(
    iteration_harness: &IterationHarness,
    coverage_harness: &mut CoverageHarness,
    baseline: Option<&BTreeMap<UCInstructionAddress, CoverageEntry>>,
    mut func: F,
) -> BTreeMap<UCInstructionAddress, CoverageEntry> {
    let itd = &interface_definition::COM_INTERFACE_DESCRIPTION;

    let results = iteration_harness.execute(
        |chunk| coverage_harness.execute(chunk, || func()),
        itd.max_number_of_hooks as usize,
    );

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

    coverage
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
