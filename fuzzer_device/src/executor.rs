mod coverage_collection;
mod hypervisor;

use crate::cmos::NMIGuard;
use crate::heuristic::Sample;
use alloc::vec::Vec;
use log::{warn};
use ::hypervisor::error::HypervisorError;
use data_types::addresses::UCInstructionAddress;
use crate::executor::coverage_collection::CoverageCollector;
use crate::executor::hypervisor::Hypervisor;

pub struct SampleExecutor {
    hypervisor: Hypervisor,
    coverage_collector: Option<CoverageCollector>,
}

impl SampleExecutor {
    pub fn execute_sample(&mut self, sample: &Sample, execution_result: &mut ExecutionResult) {
        // try to disable Non-Maskable Interrupts
        let nmi_guard = NMIGuard::disable_nmi(true);

        // prepare hypervisor for execution
        self.hypervisor.prepare_vm_state(&sample.code_blob);
        execution_result.coverage.clear();

        todo!("execute_sample");

        // re-enable Non-Maskable Interrupts
        drop(nmi_guard);
    }

    pub fn new() -> Result<Self, HypervisorError> {
        let hypervisor = Hypervisor::new()?;

        let coverage_collector = coverage_collection::CoverageCollector::initialize().map(Option::Some).unwrap_or_else(|error| {
            warn!("Failed to initialize custom processing unit: {:?}", error);
            warn!("Coverage collection will be disabled");
            None
        });

        Ok(Self {
            hypervisor,
            coverage_collector,
        })
    }

    pub fn selfcheck(&mut self) -> bool {
        self.hypervisor.selfcheck()
    }


}

#[derive(Default)]
pub struct ExecutionResult {
    pub coverage: Vec<UCInstructionAddress>,
}

