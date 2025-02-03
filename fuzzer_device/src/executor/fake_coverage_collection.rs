use alloc::collections::BTreeSet;
use coverage::harness::coverage_harness::hookable_address_iterator::HookableAddressIterator;
use coverage::harness::coverage_harness::modification_engine::ModificationEngineSettings;
use coverage::harness::coverage_harness::{
    CoverageError, CoverageExecutionResult, ExecutionResultEntry,
};
use coverage::harness::iteration_harness::IterationHarness;
use data_types::addresses::{Address, UCInstructionAddress};
use uefi::{println};

// Bochs stub for coverage collection
pub struct CoverageCollector<'a> {
    excluded_addresses: &'a BTreeSet<usize>,
}

impl<'a> CoverageCollector<'a> {
    pub fn initialize(
        excluded_addresses: &'a BTreeSet<usize>,
    ) -> Result<CoverageCollector<'a>, custom_processing_unit::Error> {
        Ok(Self { excluded_addresses })
    }

    pub fn get_iteration_harness(&self) -> IterationHarness {
        let hookable_addresses = HookableAddressIterator::construct(
            &ucode_dump::dump::ROM_cpu_000506CA,
            &ModificationEngineSettings::default(),
            0x2000,
            |x| !self.excluded_addresses.contains(&x.address()),
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
