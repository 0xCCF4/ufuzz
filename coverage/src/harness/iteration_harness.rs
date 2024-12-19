use crate::harness::coverage_harness::hookable_address_iterator::HookableAddressIterator;
use crate::harness::coverage_harness::{CoverageError, CoverageExecutionResult};
use crate::interface_definition::CoverageEntry;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use data_types::addresses::UCInstructionAddress;

pub struct IterationHarness {
    address_iterator: HookableAddressIterator,
}

impl IterationHarness {
    pub fn new(address_iterator: HookableAddressIterator) -> Self {
        IterationHarness { address_iterator }
    }

    pub fn execute<FuncResult, F: FnMut(&[UCInstructionAddress]) -> FuncResult>(
        &self,
        mut func: F,
        chunks_size: usize,
    ) -> Vec<FuncResult> {
        let mut result = Vec::with_capacity(self.address_iterator.len().div_ceil(chunks_size));

        for chunk in self.address_iterator.iter().chunks(chunks_size) {
            result.push(func(chunk));
        }

        result
    }

    pub fn execute_collect_coverage<
        DeepResult,
        FuncResult: Into<Result<CoverageExecutionResult<DeepResult>, CoverageError>>,
        F: FnMut(&[UCInstructionAddress]) -> FuncResult,
    >(
        &self,
        mut func: F,
        chunks_size: usize,
    ) -> Result<BTreeMap<UCInstructionAddress, CoverageEntry>, CoverageError> {
        let mut coverage = BTreeMap::new();

        for chunk in self.address_iterator.iter().chunks(chunks_size) {
            for hook in func(chunk).into()?.hooks {
                if hook.covered() {
                    coverage.insert(hook.address(), hook.coverage());
                }
            }
        }

        Ok(coverage)
    }
}
