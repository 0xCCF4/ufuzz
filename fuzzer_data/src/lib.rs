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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum OTADeviceToController {
    Alive {
        timestamp: u64,
        current_iteration: u64,
    },
    Blacklisted {
        address: Option<u16>,
    },
    FinishedGeneticFuzzing,
    Capabilities {
        coverage_collection: bool,
        manufacturer: String,
        processor_version_eax: u32, // cpuid 1:eax
        processor_version_ebx: u32, // cpuid 1:ebx
        processor_version_ecx: u32, // cpuid 1:ecx
        processor_version_edx: u32, // cpuid 1:edx
    },
    Ack(Box<OTAControllerToDevice>),
}

impl OTADeviceToController {
    pub fn requires_ack(&self) -> bool {
        match self {
            OTADeviceToController::Alive { .. } => false,
            OTADeviceToController::Blacklisted { .. } => true,
            OTADeviceToController::FinishedGeneticFuzzing { .. } => true,
            OTADeviceToController::Capabilities { .. } => true,
            OTADeviceToController::Ack(_) => false,
        }
    }

    pub fn ack(&self) -> Option<OTAControllerToDevice> {
        if self.requires_ack() {
            Some(OTAControllerToDevice::Ack(Box::new(self.clone())))
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum OTAControllerToDevice {
    GetCapabilities,
    Blacklist {
        address: Vec<u16>,
    },
    DidYouExcludeAnAddressLastRun,
    StartGeneticFuzzing {
        seed: u64,
        evolutions: u64,

        population_size: usize,
        code_size: usize,

        random_solutions_each_generation: usize,
        keep_best_x_solutions: usize,
        random_mutation_chance: f64,
    },
    Reboot,
    NOP,
    Ack(Box<OTADeviceToController>),
}

impl OTAControllerToDevice {
    pub fn requires_ack(&self) -> bool {
        match self {
            OTAControllerToDevice::GetCapabilities => true,
            OTAControllerToDevice::Blacklist { .. } => true,
            OTAControllerToDevice::StartGeneticFuzzing { .. } => true,
            OTAControllerToDevice::DidYouExcludeAnAddressLastRun => true,
            OTAControllerToDevice::Reboot => true,
            OTAControllerToDevice::NOP => false,
            OTAControllerToDevice::Ack(_) => false,
        }
    }

    pub fn ack(&self) -> Option<OTADeviceToController> {
        if self.requires_ack() {
            Some(OTADeviceToController::Ack(Box::new(self.clone())))
        } else {
            None
        }
    }
}
