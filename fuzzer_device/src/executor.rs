//! Sample Execution Module
//!
//! This module provides functionality for execute a fuzzing input

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
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::fmt::Debug;
use coverage::harness::coverage_harness::{CoverageExecutionResult, ExecutionResultEntry};
use coverage::harness::iteration_harness::IterationHarness;
use coverage::interface_definition::{ComInterfaceDescription, CoverageCount};
use custom_processing_unit::lmfence;
use data_types::addresses::{Address, UCInstructionAddress};
use fuzzer_data::{MemoryAccess, ReportExecutionProblem};
use log::trace;
#[cfg(feature = "__debug_print_progress_net")]
use log::Level;
use log::{error, info, warn};
#[cfg(feature = "__debug_performance_trace")]
use performance_timing::track_time;
use rand_core::RngCore;
#[cfg(feature = "__debug_print_progress_print")]
use uefi::print;
#[cfg(feature = "__debug_print_progress_print")]
use uefi::println;

/// Internal structure for managing coverage collection state
struct CoverageCollectorData {
    /// Collector for gathering coverage information
    pub collector: CoverageCollector,
    /// Planner for coverage execution on hookable addresses
    pub planner: IterationHarness,
}

/// Main executor for running and analyzing code samples
///
/// This structure manages the execution of code samples in a virtualized environment,
/// handling coverage collection, state tracking, and result analysis.
pub struct SampleExecutor {
    /// Hypervisor instance for virtualized execution
    hypervisor: Hypervisor,
    /// Optional coverage collection state
    coverage: Option<CoverageCollectorData>,
    /// Serializer for code samples
    serializer: Serializer,
    /// Description of the coverage interface
    coverage_interface: &'static ComInterfaceDescription,
}

fn disable_all_hooks() {
    lmfence();
    #[cfg(not(feature = "__debug_bochs_pretend"))]
    custom_processing_unit::disable_all_hooks();
}

/// Result of executing a single code sample
pub struct ExecutionSampleResult {
    /// Serialized version of the executed sample, if successful
    pub serialized_sample: Option<Vec<u8>>,
}

impl SampleExecutor {
    /// Checks if coverage collection is supported
    pub fn supports_coverage_collection(&self) -> bool {
        self.coverage.is_some()
    }

    /// Executes a code sample and collects execution results
    ///
    /// # Arguments
    ///
    /// * `sample` - The code sample to execute
    /// * `execution_result` - Where to store execution results
    /// * `cmos` - CMOS storage for persistent data
    /// * `random` - Random number generator for serialization
    /// * `net` - Optional network connection for logging
    ///
    /// # Returns
    ///
    /// * `ExecutionSampleResult` the results of the execution
    #[cfg_attr(feature = "__debug_performance_trace", track_time)]
    #[allow(unused_mut, unused_variables)]
    pub fn execute_sample<R: RngCore>(
        &mut self,
        sample: &[u8],
        execution_result: &mut ExecutionResult,
        cmos: &mut cmos::CMOS<PersistentApplicationData>,
        random: &mut R,
        mut net: Option<&mut ControllerConnection>,
        collect_coverage: bool,
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
                let no_coverage_vm_exit = self.hypervisor.run_vm(false);

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

        if let VmExitReason::EPTPageFault(fault) = &execution_result.exit {
            if ((fault.data_read && !fault.was_readable)
                || (fault.data_write && !fault.was_writable))
                && fault.gpa >= self.coverage_interface.base as usize
                && fault.gpa < self.coverage_interface.base as usize + 4096
            {
                // read, write to coverage collection region
                info!("read or write to coverage collection region");

                execution_result
                    .events
                    .push(ExecutionEvent::AccessCoverageArea);

                return ExecutionSampleResult {
                    serialized_sample: None,
                };
            }
        }

        if let Some(coverage) = self.coverage.as_mut() {
            if collect_coverage {
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
                                            self.hypervisor.run_with_callback(true, disable_all_hooks);
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
                            result: (hooked_addresses, mut current_vm_exit, current_vm_state),
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

                            if let VmExitReason::EPTPageFault(fault) = &mut current_vm_exit {
                                if fault.gpa >= self.coverage_interface.base as usize
                                    && fault.gpa < (self.coverage_interface.base as usize + 4096)
                                {
                                    // since in normal execution this is the setting
                                    fault.was_writable = false;
                                    fault.was_readable = false;
                                }
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
                    let serialized_vm_exit = self.hypervisor.run_vm(false);

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

    /// Creates a new sample executor instance
    ///
    /// # Arguments
    ///
    /// * `excluded_addresses` - Set of addresses to exclude from coverage
    /// * `coverage_interface` - Description of the coverage interface
    ///
    /// # Returns
    ///
    /// * `Result<SampleExecutor, HypervisorError>` - New executor instance or error
    pub fn new(
        excluded_addresses: Rc<RefCell<BTreeSet<u16>>>,
        coverage_interface: &'static ComInterfaceDescription,
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
        let hypervisor = Hypervisor::new(coverage_interface)?;
        trace!("Hypervisor initialized");

        Ok(Self {
            hypervisor,
            coverage: coverage_collector,
            serializer: Serializer::default(),
            coverage_interface,
        })
    }

    /// Updates the set of excluded addresses for coverage collection
    pub fn update_excluded_addresses(&mut self) {
        if let Some(coverage) = self.coverage.as_mut() {
            coverage.planner = coverage.collector.get_iteration_harness();
        }
    }

    /// Traces the execution of a code sample
    ///
    /// # Arguments
    ///
    /// * `sample` - The code sample to trace
    /// * `trace_result` - Where to store the trace
    /// * `max_trace_length` - Maximum number of instructions to trace
    ///
    /// # Returns
    ///
    /// * `VmExitReason` - Reason for VM exit
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

    /// Traces the state changes during sample execution
    ///
    /// # Arguments
    ///
    /// * `sample` - The code sample to trace
    /// * `trace_result` - Where to store the state trace
    /// * `max_trace_length` - Maximum number of states to trace
    ///
    /// # Returns
    ///
    /// * `VmExitReason` - Reason for VM exit
    pub fn state_trace_sample(
        &mut self,
        sample: &[u8],
        trace_result: &mut StateTrace<VmState>,
        max_trace_length: usize,
    ) -> VmExitReason {
        self.hypervisor.load_code_blob(sample);
        self.hypervisor.prepare_vm_state();
        self.hypervisor
            .state_trace_vm(trace_result, max_trace_length)
    }

    /// Traces the state changes during sample execution and record memory accesses
    ///
    /// # Arguments
    ///
    /// * `sample` - The code sample to trace
    /// * `trace_result` - Where to store the state trace
    /// * `max_trace_length` - Maximum number of states to trace
    ///
    /// # Returns
    ///
    /// * `VmExitReason` - Reason for VM exit
    pub fn state_trace_sample_mem(
        &mut self,
        sample: &[u8],
        trace_result: &mut StateTrace<(VmState, Vec<MemoryAccess>)>,
        max_trace_length: usize,
    ) -> VmExitReason {
        self.hypervisor.load_code_blob(sample);
        self.hypervisor.prepare_vm_state();
        self.hypervisor
            .state_trace_vm_memory_introspection(trace_result, max_trace_length)
    }

    /// Performs a self-check of the executor
    ///
    /// # Returns
    ///
    /// * `bool` - True if self-check passed
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

/// Events that can occur during code sample execution
#[derive(Debug, Clone)]
pub enum ExecutionEvent {
    /// VM state mismatch during coverage collection
    VmMismatchCoverageCollection {
        /// Address where mismatch occurred
        address: u16,
        /// Exit reason from coverage collection, set if different from normal execution
        coverage_exit: Option<VmExitReason>,
        /// State during coverage collection, set if different from normal execution
        coverage_state: Option<VmState>,
    },
    /// Error during coverage collection
    CoverageCollectionError {
        /// The error that occurred
        error: coverage::harness::coverage_harness::CoverageError,
    },
    /// Mismatch between serialized and normal execution
    SerializedMismatch {
        /// Exit reason from serialized execution, set if different from normal execution
        serialized_exit: Option<VmExitReason>,
        /// State from serialized execution, set if different from normal execution
        serialized_state: Option<VmState>,
    },
    /// Indicates a likely bug in the code
    VeryLikelyBug,
    /// Mismatch in state trace
    StateTraceMismatch {
        /// Index where mismatch occurred
        index: u64,
        /// State from normal execution, set if different from serialized execution
        normal: Option<VmState>,
        /// State from serialized execution, set if different from normal execution
        serialized: Option<VmState>,
    },
    /// Access to coverage collection area
    AccessCoverageArea,
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
            ExecutionEvent::AccessCoverageArea => Some(ReportExecutionProblem::AccessCoverageArea),
        }
    }
}

/// Tracks the results of executing a code sample
#[derive(Default)]
pub struct ExecutionResult {
    /// Coverage information for each instruction address
    pub coverage: BTreeMap<UCInstructionAddress, CoverageCount>,
    /// Events that occurred during execution
    pub events: Vec<ExecutionEvent>,
    /// Final VM state
    pub state: VmState,
    /// Reason for VM exit
    pub exit: VmExitReason,
    /// Trace of executed instructions
    pub trace: Trace,
}

impl ExecutionResult {
    /// Resets the execution result to initial state
    ///
    /// # Arguments
    ///
    /// * `initial_state` - Initial VM state to use
    pub fn reset(&mut self, initial_state: &VmState) {
        self.coverage.clear();
        self.events.clear();
        self.state.clone_from(initial_state);
        self.trace.clear();
        self.exit = VmExitReason::default();
    }
}
