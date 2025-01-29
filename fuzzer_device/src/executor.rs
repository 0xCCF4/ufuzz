mod coverage_collection;
mod hypervisor;

use alloc::collections::{btree_map, BTreeMap};
use crate::cmos::NMIGuard;
use crate::executor::coverage_collection::CoverageCollector;
use crate::executor::hypervisor::Hypervisor;
use crate::heuristic::Sample;
use ::hypervisor::error::HypervisorError;
use alloc::vec::Vec;
use data_types::addresses::UCInstructionAddress;
use log::{error, warn};
use uefi::println;
use ::hypervisor::hardware_vt::VmExitReason;
use ::hypervisor::state::VmState;
use coverage::harness::coverage_harness::{CoverageExecutionResult, ExecutionResultEntry};
use coverage::harness::iteration_harness::{IterationHarness};
use coverage::interface_definition::CoverageCount;

struct CoverageCollectorData {
    pub collector: CoverageCollector,
    pub planner: IterationHarness,
}

pub struct SampleExecutor {
    hypervisor: Hypervisor,
    coverage: Option<CoverageCollectorData>,

    state_scratch_area: VmState,
}

fn disable_all_hooks() {
    custom_processing_unit::disable_all_hooks();
}

impl SampleExecutor {
    pub fn execute_sample<'a>(&'a mut self, sample: &Sample, execution_result: &mut ExecutionResult) {
        // try to disable Non-Maskable Interrupts
        let nmi_guard = NMIGuard::disable_nmi(true);

        execution_result.reset(&self.hypervisor.initial_state);

        if let Some(coverage) = self.coverage.as_mut() {
            // plan the following executions to collect coverage
            let execution_plan = coverage.planner.execute_for_all_addresses(|addresses: &[UCInstructionAddress]| {

                // prepare hypervisor for execution
                self.hypervisor.prepare_vm_state(&sample.code_blob);

                coverage.collector.execute_coverage_collection(addresses, || {

                    // execute the sample within the hypervisor
                    let vm_exit = self.hypervisor.run_with_callback(disable_all_hooks);
                    // hooks are now disabled

                    let mut state = VmState::default();
                    self.hypervisor.capture_state(&mut state);

                    let addresses: Vec<UCInstructionAddress> = addresses.to_vec(); // todo optimize

                    (addresses, vm_exit, state) // disable hooks after execution to not capture conditional logic of the vm exit handler
                })
            });

            let mut expected_vm_exit = None;

            // execute the planned coverage collection
            for iteration in execution_plan {
                // save the vm current state


                match iteration {
                    Err(error) => {
                        error!("Failed to collect coverage: {:?}. This should not have happened.", error);
                        execution_result.events.push(ExecutionEvent::CoverageCollectionError {
                            error,
                        });
                    }
                    Ok(CoverageExecutionResult {result: (hooked_addresses, current_vm_exit, current_vm_state), hooks: coverage_information}) => {
                        // first check if the result of the execution is the same for the current iteration
                        match expected_vm_exit {
                            None => {
                                expected_vm_exit = Some(current_vm_exit);
                                self.state_scratch_area.clone_from(&current_vm_state);
                            }
                            Some(ref expected_vm_exit) => {
                                if expected_vm_exit != &current_vm_exit {
                                    error!("Expected result {:?} but got {:?}. This should not have happened. This is a µcode bug or implementation problem!", expected_vm_exit, current_vm_exit);
                                    println!("We were hooking the following addresses:");
                                    for address in hooked_addresses.iter() {
                                        println!(" - {:?}", address);
                                    }
                                    execution_result.events.push(ExecutionEvent::VmExitMismatchCoverageCollection {
                                        addresses: hooked_addresses.to_vec(),
                                        expected_exit: expected_vm_exit.clone(),
                                        actual_exit: current_vm_exit,
                                    });
                                }
                                if self.state_scratch_area != execution_result.state {
                                    error!("Expected architectural state x but got y. This should not have happened. This is a µcode bug or implementation problem!");
                                    println!("We were hooking the following addresses:");
                                    for address in hooked_addresses.iter() {
                                        println!(" - {:?}", address);
                                    }
                                    execution_result.events.push(ExecutionEvent::VmStateMismatchCoverageCollection {
                                        addresses: hooked_addresses.to_vec(),
                                        expected_exit: self.state_scratch_area.clone(),
                                        actual_exit: execution_result.state.clone(),
                                    });
                                }
                            }
                        }

                        // extract the coverage information
                        for ucode_location in coverage_information {
                            if let ExecutionResultEntry::Covered {address, count} = ucode_location {
                                let entry = execution_result.coverage.entry(ucode_location.address());

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
            self.hypervisor.prepare_vm_state(&sample.code_blob);

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
            state_scratch_area: hypervisor.initial_state.clone(),
            hypervisor,
            coverage: coverage_collector,
        })
    }

    pub fn selfcheck(&mut self) -> bool {
        self.hypervisor.selfcheck()
    }
}

pub enum ExecutionEvent {
    VmExitMismatchCoverageCollection {
        addresses: Vec<UCInstructionAddress>,
        expected_exit: VmExitReason,
        actual_exit: VmExitReason,
    },
    VmStateMismatchCoverageCollection {
        addresses: Vec<UCInstructionAddress>,
        expected_exit: VmState,
        actual_exit: VmState,
    },
    CoverageCollectionError {
        error: coverage::harness::coverage_harness::CoverageError,
    }
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
