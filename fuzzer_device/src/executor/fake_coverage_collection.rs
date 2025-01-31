use alloc::boxed::Box;
use alloc::format;
use alloc::string::ToString;
use core::ops::{Deref, DerefMut};
use core::pin::Pin;
use coverage::harness::coverage_harness::hookable_address_iterator::HookableAddressIterator;
use coverage::harness::coverage_harness::modification_engine::ModificationEngineSettings;
use coverage::harness::coverage_harness::{
    CoverageError, CoverageExecutionResult, CoverageHarness, ExecutionResultEntry,
};
use coverage::harness::iteration_harness::IterationHarness;
use coverage::interface::safe::ComInterface;
use coverage::interface_definition;
use custom_processing_unit::CustomProcessingUnit;
use data_types::addresses::UCInstructionAddress;
use log::trace;
use ucode_dump::RomDump;
use uefi::{print, println};

// Bochs stub for coverage collection
pub struct CoverageCollector {}

impl CoverageCollector {
    pub fn initialize() -> Result<Self, custom_processing_unit::Error> {
        Ok(Self {})
    }

    pub fn get_iteration_harness(&self) -> IterationHarness {
        let hookable_addresses = HookableAddressIterator::construct(
            &ucode_dump::dump::ROM_cpu_000506CA,
            &ModificationEngineSettings::default(),
            0x2000,
        );

        IterationHarness::new(hookable_addresses)
    }

    pub fn execute_coverage_collection<FuncResult, F: FnOnce() -> FuncResult>(
        &mut self,
        hooks: &[UCInstructionAddress],
        func: F,
    ) -> Result<CoverageExecutionResult<FuncResult>, CoverageError> {
        println!(" --> Executing coverage collection");
        Ok(CoverageExecutionResult {
            result: func(),
            hooks: hooks
                .iter()
                .map(|x| ExecutionResultEntry::NotCovered { address: *x })
                .collect(),
        })
    }
}
