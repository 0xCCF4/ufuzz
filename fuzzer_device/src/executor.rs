#[cfg(not(feature = "__debug_bochs_pretend"))]
mod coverage_collection;
#[cfg(feature = "__debug_bochs_pretend")]
mod fake_coverage_collection;
#[cfg(feature = "__debug_bochs_pretend")]
use fake_coverage_collection as coverage_collection;
mod hypervisor;

use crate::cmos::NMIGuard;
use crate::executor::coverage_collection::CoverageCollector;
use crate::executor::hypervisor::Hypervisor;
use crate::heuristic::Sample;
use ::hypervisor::error::HypervisorError;
use ::hypervisor::hardware_vt::VmExitReason;
use ::hypervisor::state::VmState;
use alloc::collections::{btree_map, BTreeMap};
use alloc::vec::Vec;
use core::fmt::Debug;
use coverage::harness::coverage_harness::{CoverageExecutionResult, ExecutionResultEntry};
use coverage::harness::iteration_harness::IterationHarness;
use coverage::interface_definition::CoverageCount;
use data_types::addresses::UCInstructionAddress;
use log::{error, warn};

struct CoverageCollectorData {
    pub collector: CoverageCollector,
    pub planner: IterationHarness,
}

pub struct SampleExecutor {
    hypervisor: Hypervisor,
    coverage: Option<CoverageCollectorData>,
}

fn disable_all_hooks() {
    #[cfg(not(feature = "__debug_bochs_pretend"))]
    custom_processing_unit::disable_all_hooks();
}

impl SampleExecutor {
    pub fn execute_sample<'a>(
        &'a mut self,
        sample: &Sample,
        execution_result: &mut ExecutionResult,
    ) {
        // try to disable Non-Maskable Interrupts
        let nmi_guard = NMIGuard::disable_nmi(true);

        execution_result.reset(&self.hypervisor.initial_state);

        // load code sample to hypervisor memory
        self.hypervisor.load_code_blob(&sample.code_blob);

        if let Some(coverage) = self.coverage.as_mut() {
            // plan the following executions to collect coverage
            let execution_plan = coverage.planner.execute_for_all_addresses(
                |addresses: &[UCInstructionAddress]| {
                    // prepare hypervisor for execution
                    self.hypervisor.prepare_vm_state();

                    coverage
                        .collector
                        .execute_coverage_collection(addresses, || {
                            // execute the sample within the hypervisor
                            let vm_exit = self.hypervisor.run_with_callback(disable_all_hooks);
                            // hooks are now disabled, so coverage collection stopped

                            let mut state = self.hypervisor.initial_state.clone();
                            self.hypervisor.capture_state(&mut state);

                            let addresses: Vec<UCInstructionAddress> = addresses.to_vec(); // todo optimize

                            (addresses, vm_exit, state) // disable hooks after execution to not capture conditional logic of the vm exit handler
                        })
                },
            );

            // since, we are executing the sample multiple times, we would expect the same output each time
            let mut expected_vm_exit = None;
            let mut expected_vm_state = None;

            // execute the planned coverage collection
            for iteration in execution_plan {
                // save the vm current state

                match iteration {
                    Err(error) => {
                        execution_result
                            .events
                            .push(ExecutionEvent::CoverageCollectionError { error });
                    }
                    Ok(CoverageExecutionResult {
                        result: (hooked_addresses, current_vm_exit, current_vm_state),
                        hooks: coverage_information,
                    }) => {
                        // first check if the result of the execution is the same for the current iteration
                        match (&mut expected_vm_exit, &mut expected_vm_state) {
                            (None, None) => {
                                expected_vm_exit = Some(current_vm_exit);
                                expected_vm_state = Some(current_vm_state);
                            }
                            (Some(ref expected_vm_exit), Some(ref expected_vm_state)) => {
                                if expected_vm_exit != &current_vm_exit {
                                    execution_result.events.push(
                                        ExecutionEvent::VmExitMismatchCoverageCollection {
                                            addresses: hooked_addresses.to_vec(),
                                            expected_exit: expected_vm_exit.clone(),
                                            actual_exit: current_vm_exit.clone(),
                                        },
                                    );
                                }
                                if expected_vm_state != &current_vm_state {
                                    execution_result.events.push(
                                        ExecutionEvent::VmStateMismatchCoverageCollection {
                                            addresses: hooked_addresses.to_vec(),
                                            exit: current_vm_exit,
                                            expected_state: expected_vm_state.clone(),
                                            actual_state: current_vm_state,
                                        },
                                    );
                                }
                            }
                            _ => unreachable!("both are set simultaneously"),
                        }

                        // extract the coverage information
                        for ucode_location in coverage_information {
                            if let ExecutionResultEntry::Covered { address, count } = ucode_location
                            {
                                let entry =
                                    execution_result.coverage.entry(ucode_location.address());

                                match entry {
                                    btree_map::Entry::Occupied(mut entry) => {
                                        error!("Address {:?} was already analyzed for coverage. This should not have happened. This is an implementation problem!", address);
                                        *entry.get_mut() = count; // just replace the count
                                    }
                                    btree_map::Entry::Vacant(entry) => {
                                        entry.insert(count);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        } else {
            // prepare hypervisor for execution
            self.hypervisor.prepare_vm_state();

            // run the hypervisor
            let _vm_exit = self.hypervisor.run_vm();

            // save the vm current state
            self.hypervisor.capture_state(&mut execution_result.state);
        }

        // re-enable Non-Maskable Interrupts
        drop(nmi_guard);
    }

    pub fn new() -> Result<Self, HypervisorError> {
        let hypervisor = Hypervisor::new()?;

        let coverage_collector = coverage_collection::CoverageCollector::initialize()
            .map(Option::Some)
            .unwrap_or_else(|error| {
                warn!("Failed to initialize custom processing unit: {:?}", error);
                warn!("Coverage collection will be disabled");
                None
            })
            .map(|collector| {
                let hookable_addresses = collector.get_iteration_harness();
                CoverageCollectorData {
                    collector: collector,
                    planner: hookable_addresses,
                }
            });

        Ok(Self {
            hypervisor,
            coverage: coverage_collector,
        })
    }

    pub fn selfcheck(&mut self) -> bool {
        self.hypervisor.selfcheck()
    }
}

#[derive(Debug)]
pub enum ExecutionEvent {
    VmExitMismatchCoverageCollection {
        addresses: Vec<UCInstructionAddress>,
        expected_exit: VmExitReason,
        actual_exit: VmExitReason,
    },
    VmStateMismatchCoverageCollection {
        addresses: Vec<UCInstructionAddress>,
        exit: VmExitReason,
        expected_state: VmState,
        actual_state: VmState,
    },
    CoverageCollectionError {
        error: coverage::harness::coverage_harness::CoverageError,
    },
}

#[derive(Default)]
pub struct ExecutionResult {
    pub coverage: BTreeMap<UCInstructionAddress, CoverageCount>, // mapping address -> count
    pub events: Vec<ExecutionEvent>,
    pub state: VmState,
}

impl ExecutionResult {
    pub fn reset(&mut self, initial_state: &VmState) {
        self.coverage.clear();
        self.events.clear();
        self.state.clone_from(initial_state);
    }
}
