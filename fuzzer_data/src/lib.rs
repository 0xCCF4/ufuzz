#![no_std]

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use hypervisor::state::{VmExitReason, VmState};
use serde::{Deserialize, Serialize};

extern crate alloc;

#[derive(Debug, Serialize, Deserialize)]
pub struct ExecutionExit {
    pub exit_reason: VmExitReason,
    pub state: VmState,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SampleExecutionResult {
    pub coverage: Option<BTreeMap<usize, usize>>, // mapping address -> count

    pub normal_execution: ExecutionExit,
    pub traced_execution: Option<ExecutionExit>, // None if not different to normal
    pub serialized_execution: Option<ExecutionExit>, // None if not different to normal
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SampleRunResult {
    pub code: Vec<u8>,
    pub result: SampleExecutionResult,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Sample {
    pub code: Vec<u8>,

    pub results: BTreeMap<String, SampleExecutionResult>, // device -> result
}

#[derive(Debug, Serialize, Deserialize)]
pub enum OTADeviceToController {
    Alive {
        timestamp: u64,
        current_iteration: u64,
    },
    Blacklisted {
        address: Vec<usize>,
    },
    FinishedGeneticFuzzing {
        coverage: Vec<usize>,
    },
    Capabilities {
        coverage_collection: bool,
        manufacturer: String,
        processor_version_eax: u64, // cpuid 1:eax
        processor_version_ebx: u64, // cpuid 1:ebx
        processor_version_ecx: u64, // cpuid 1:ecx
        processor_version_edx: u64, // cpuid 1:edx
    },
    Ack(Box<OTAControllerToDevice>),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum OTAControllerToDevice {
    GetCapabilities,
    Blacklist {
        address: Vec<usize>,
    },
    StartGeneticFuzzing {
        seed: u64,
        evolutions: u64,

        population_size: usize,
        code_size: usize,

        random_solutions_each_generation: usize,
        keep_best_x_solutions: usize,
        random_mutation_chance: f64,
    },
    NOP,
    Ack(Box<OTADeviceToController>),
}
