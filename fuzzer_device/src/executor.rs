use crate::cmos::NMIGuard;
use crate::heuristic::Sample;
use alloc::vec::Vec;
use data_types::addresses::UCInstructionAddress;

#[derive(Default)]
pub struct SampleExecutor {}

impl SampleExecutor {
    pub fn execute_sample(&mut self, sample: &Sample, execution_result: &mut ExecutionResult) {
        let nmi_guard = NMIGuard::disable_nmi(true);

        drop(nmi_guard);
    }
}

#[derive(Default)]
pub struct ExecutionResult {
    pub coverage: Vec<UCInstructionAddress>,
}
