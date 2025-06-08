#[cfg(not(feature = "__debug_bochs_pretend"))]
mod coverage_collection;
#[cfg(feature = "__debug_bochs_pretend")]
mod fake_coverage_collection;
#[cfg(feature = "__debug_bochs_pretend")]
use fake_coverage_collection as coverage_collection;
pub mod hypervisor;

use crate::cmos::NMIGuard;
use crate::controller_connection::ControllerConnection;
#[cfg(feature = "__debug_print_dissassembly")]
use crate::disassemble_code;
use crate::executor::coverage_collection::CoverageCollector;
use crate::executor::hypervisor::Hypervisor;
use crate::mutation_engine::serialize::{SerializeError, Serializer};
use crate::{cmos, PersistentApplicationData, PersistentApplicationState, StateTrace, Trace};
use ::hypervisor::error::HypervisorError;
use ::hypervisor::state::{VmExitReason, VmState};
use alloc::collections::{btree_map, BTreeMap, BTreeSet};
use alloc::format;
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::fmt::Debug;
use coverage::harness::coverage_harness::{CoverageExecutionResult, ExecutionResultEntry};
use coverage::harness::iteration_harness::IterationHarness;
use coverage::interface_definition::CoverageCount;
use custom_processing_unit::lmfence;
use data_types::addresses::{Address, UCInstructionAddress};
use fuzzer_data::ReportExecutionProblem;
use log::{error, warn};
use log::trace;
#[cfg(feature = "__debug_print_progress_net")]
use log::{Level};
#[cfg(feature = "__debug_performance_trace")]
use performance_timing::track_time;
use rand_core::RngCore;
#[cfg(feature = "__debug_print_progress_print")]
use uefi::print;
#[cfg(feature = "__debug_print_progress_print")]
use uefi::println;

struct CoverageCollectorData {
    pub collector: CoverageCollector,
    pub planner: IterationHarness,
}

pub struct SampleExecutor {
    hypervisor: Hypervisor,
    coverage: Option<CoverageCollectorData>,
    serializer: Serializer,
}

fn disable_all_hooks() {
    lmfence();
    #[cfg(not(feature = "__debug_bochs_pretend"))]
    custom_processing_unit::disable_all_hooks();
}

pub struct ExecutionSampleResult {
    pub serialized_sample: Option<Vec<u8>>,
}

impl SampleExecutor {
    pub fn supports_coverage_collection(&self) -> bool {
        self.coverage.is_some()
    }

    #[cfg_attr(feature = "__debug_performance_trace", track_time)]
    #[allow(unused_mut, unused_variables)]
    pub fn execute_sample<R: RngCore>(
        &mut self,
        sample: &[u8],
        execution_result: &mut ExecutionResult,
        cmos: &mut cmos::CMOS<PersistentApplicationData>,
        random: &mut R,
        mut net: Option<&mut ControllerConnection>,
    ) -> ExecutionSampleResult {
        // try to disable Non-Maskable Interrupts
        let nmi_guard = NMIGuard::disable_nmi(true);

        execution_result.reset(&self.hypervisor.initial_state);

        // load code sample to hypervisor memory
        self.hypervisor.load_code_blob(sample);

        #[cfg(feature = "__debug_print_dissassembly")]
        disassemble_code(sample);
        #[cfg(feature = "__debug_print_dissassembly")]
        println!();

        // do a trace
        self.hypervisor.prepare_vm_state();
        self.hypervisor.trace_vm(&mut execution_result.trace, 500);

        let serialized_sample = self
            .serializer
            .serialize_code(random, sample, &execution_result.trace)
            .map(Option::Some)
            .unwrap_or_else(|e| {
                if !matches!(e, SerializeError::IndirectBranch) {
                    error!("Failed to serialize sample: {e}");
                    crate::disassemble_code(sample);
                    error!("--------------------- END OF DISASSEMBLY ---------------------");
                }
                None
            });

        // do a normal sample execution
        let no_coverage_vm_exit = {
            let mut iteration = 0;
            loop {
                iteration += 1;

                self.hypervisor.prepare_vm_state();
                let no_coverage_vm_exit = self.hypervisor.run_vm();

                if iteration < 100 && no_coverage_vm_exit == VmExitReason::ExternalInterrupt {
                    #[cfg(feature = "__debug_print_external_interrupt_notification")]
                    trace!(
                        "External interrupt detected (serialized). Retrying... {}",
                        iteration
                    );
                    continue;
                } else {
                    break no_coverage_vm_exit;
                }
            }
        };

        self.hypervisor.capture_state(&mut execution_result.state);

        execution_result.exit = no_coverage_vm_exit;
        let _no_coverage_execution_result = &execution_result.state;
        let _no_coverage_execution_exit = &execution_result.exit;

        if let Some(coverage) = self.coverage.as_mut() {
            // plan the following executions to collect coverage
            let execution_plan = coverage.planner.execute_for_all_addresses(
                |addresses: &[UCInstructionAddress]| {
                    if addresses.len() == 1 {
                        cmos.data_mut_or_insert().state =
                            PersistentApplicationState::CollectingCoverage(
                                addresses[0].address() as u16
                            );
                    } else {
                        todo!("Implement multi-address coverage collection");
                    }

                    #[cfg(feature = "__debug_print_progress_print")]
                    print!("{}\r", addresses[0]);

                    let mut iteration: usize = 0;
                    loop {
                        iteration += 1;

                        // prepare hypervisor for execution
                        self.hypervisor.prepare_vm_state();

                        let result =
                            coverage
                                .collector
                                .execute_coverage_collection(addresses, || {
                                    // execute the sample within the hypervisor
                                    let vm_exit =
                                        self.hypervisor.run_with_callback(disable_all_hooks);
                                    // hooks are now disabled, so coverage collection stopped

                                    if addresses.len() == 1 {
                                        cmos.data_mut_or_insert().state =
                                            PersistentApplicationState::Idle;
                                    }

                                    let mut state = self.hypervisor.initial_state.clone();
                                    self.hypervisor.capture_state(&mut state);

                                    let addresses: Vec<UCInstructionAddress> = addresses.to_vec(); // todo optimize

                                    (addresses, vm_exit, state) // disable hooks after execution to not capture conditional logic of the vm exit handler
                                });

                        match result {
                            Err(error) => {
                                return Err(error);
                            }
                            Ok(result) => {
                                if result.result.1 == VmExitReason::ExternalInterrupt {
                                    #[cfg(
                                        feature = "__debug_print_external_interrupt_notification"
                                    )]
                                    trace!(
                                        "External interrupt detected. Retrying... {}",
                                        iteration
                                    );
                                    if iteration < 10 {
                                        continue;
                                    } else {
                                        return Ok(result);
                                    }
                                } else {
                                    return Ok(result);
                                }
                            }
                        }
                    }
                },
            );

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
                        #[cfg(feature = "__debug_print_progress_net")]
                        if let Some(ref mut net) = net {
                            let _ = net.log_unreliable(
                                Level::Warn,
                                format!(
                                    "Address: {:04x?}: {:x?}",
                                    hooked_addresses, coverage_information
                                ),
                            );
                        }

                        // first check if the result of the execution is the same for the current iteration
                        let exit = if &execution_result.exit != &current_vm_exit {
                            Some(current_vm_exit)
                        } else {
                            None
                        };
                        let state = if &execution_result.state != &current_vm_state {
                            Some(current_vm_state)
                        } else {
                            None
                        };

                        if exit.is_some() || state.is_some() {
                            execution_result.events.push(
                                ExecutionEvent::VmMismatchCoverageCollection {
                                    address: hooked_addresses[0].address() as u16,
                                    coverage_exit: exit,
                                    coverage_state: state,
                                },
                            );
                        }

                        // extract the coverage information
                        for ucode_location in coverage_information {
                            if let ExecutionResultEntry::Covered {
                                address,
                                count,
                                last_rip: _,
                            } = ucode_location
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

            #[cfg(feature = "__debug_print_progress_print")]
            print!("     \r");
        }

        // do a serialized execution
        if let Some(ref serialized_code) = serialized_sample {
            self.hypervisor.load_code_blob(serialized_code.as_slice());

            let serialized_vm_exit = {
                let mut iteration = 0;
                loop {
                    iteration += 1;

                    self.hypervisor.prepare_vm_state();
                    let serialized_vm_exit = self.hypervisor.run_vm();

                    if iteration < 100 && serialized_vm_exit == VmExitReason::ExternalInterrupt {
                        #[cfg(feature = "__debug_print_external_interrupt_notification")]
                        trace!(
                            "External interrupt detected (serialized). Retrying... {}",
                            iteration
                        );
                        continue;
                    } else {
                        break serialized_vm_exit;
                    }
                }
            };

            let mut serialized_state = self.hypervisor.initial_state.clone();

            self.hypervisor.capture_state(&mut serialized_state);

            let exit = if !execution_result.exit.is_same_kind(&serialized_vm_exit) {
                Some(serialized_vm_exit)
            } else {
                None
            };

            let state = if !execution_result
                .state
                .is_equal_no_address_compare(&serialized_state)
            {
                Some(serialized_state)
            } else {
                None
            };

            if exit.is_some() || state.is_some() {
                execution_result
                    .events
                    .push(ExecutionEvent::SerializedMismatch {
                        serialized_exit: exit,
                        serialized_state: state,
                    });
            }
        }

        // re-enable Non-Maskable Interrupts
        drop(nmi_guard);

        ExecutionSampleResult { serialized_sample }
    }

    pub fn new(
        excluded_addresses: Rc<RefCell<BTreeSet<u16>>>,
    ) -> Result<SampleExecutor, HypervisorError> {
        trace!("Initializing coverage collection");

        #[cfg(not(feature = "__debug_pretend_no_coverage"))]
        let coverage_collector =
            coverage_collection::CoverageCollector::initialize(excluded_addresses)
                .map(Option::Some)
                .unwrap_or_else(|error| {
                    warn!("Failed to initialize custom processing unit: {:?}", error);
                    warn!("Coverage collection will be disabled");
                    None
                })
                .map(|collector| {
                    let hookable_addresses = collector.get_iteration_harness();
                    CoverageCollectorData {
                        collector,
                        planner: hookable_addresses,
                    }
                });

        #[cfg(feature = "__debug_pretend_no_coverage")]
        let coverage_collector = None;

        trace!("Coverage collection initialized");

        trace!("Initializing hypervisor");
        let hypervisor = Hypervisor::new()?;
        trace!("Hypervisor initialized");

        Ok(Self {
            hypervisor,
            coverage: coverage_collector,
            serializer: Serializer::default(),
        })
    }

    pub fn update_excluded_addresses(&mut self) {
        if let Some(coverage) = self.coverage.as_mut() {
            coverage.planner = coverage.collector.get_iteration_harness();
        }
    }

    pub fn trace_sample(
        &mut self,
        sample: &[u8],
        trace_result: &mut Trace,
        max_trace_length: usize,
    ) -> VmExitReason {
        self.hypervisor.load_code_blob(sample);
        self.hypervisor.prepare_vm_state();
        self.hypervisor.trace_vm(trace_result, max_trace_length)
    }

    pub fn state_trace_sample(
        &mut self,
        sample: &[u8],
        trace_result: &mut StateTrace,
        max_trace_length: usize,
    ) -> VmExitReason {
        self.hypervisor.load_code_blob(sample);
        self.hypervisor.prepare_vm_state();
        self.hypervisor
            .state_trace_vm(trace_result, max_trace_length)
    }

    pub fn selfcheck(&mut self) -> bool {
        for _ in 0..10 {
            // can fail due to ExternalInterrupts
            if self.hypervisor.selfcheck() {
                return true;
            }
            warn!("Selfcheck failed...");
        }
        false
    }
}

#[derive(Debug, Clone)]
pub enum ExecutionEvent {
    VmMismatchCoverageCollection {
        address: u16,

        coverage_exit: Option<VmExitReason>,
        coverage_state: Option<VmState>,
    },
    CoverageCollectionError {
        error: coverage::harness::coverage_harness::CoverageError,
    },
    SerializedMismatch {
        serialized_exit: Option<VmExitReason>,
        serialized_state: Option<VmState>,
    },
    VeryLikelyBug,
    StateTraceMismatch {
        index: u64,
        normal: Option<VmState>,
        serialized: Option<VmState>,
    },
}

impl From<ExecutionEvent> for Option<ReportExecutionProblem> {
    fn from(value: ExecutionEvent) -> Self {
        match value {
            ExecutionEvent::SerializedMismatch {
                serialized_exit,
                serialized_state,
            } => Some(ReportExecutionProblem::SerializedMismatch {
                serialized_exit,
                serialized_state,
            }),
            ExecutionEvent::CoverageCollectionError { .. } => None,
            ExecutionEvent::VmMismatchCoverageCollection {
                address,
                coverage_exit,
                coverage_state,
            } => Some(ReportExecutionProblem::CoverageProblem {
                address,
                coverage_exit,
                coverage_state,
            }),
            ExecutionEvent::StateTraceMismatch {
                normal,
                serialized,
                index,
            } => Some(ReportExecutionProblem::StateTraceMismatch {
                index,
                normal,
                serialized,
            }),
            ExecutionEvent::VeryLikelyBug => Some(ReportExecutionProblem::VeryLikelyBug),
        }
    }
}

#[derive(Default)]
pub struct ExecutionResult {
    pub coverage: BTreeMap<UCInstructionAddress, CoverageCount>, // mapping address -> count
    pub events: Vec<ExecutionEvent>,

    pub state: VmState,
    pub exit: VmExitReason,

    pub trace: Trace,
}

impl ExecutionResult {
    pub fn reset(&mut self, initial_state: &VmState) {
        self.coverage.clear();
        self.events.clear();
        self.state.clone_from(initial_state);
        self.trace.clear();
        self.exit = VmExitReason::default();
    }
}
