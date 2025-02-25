#![no_std]

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Debug;
use hypervisor::state::{VmExitReason, VmState};
use serde::{Deserialize, Serialize};

extern crate alloc;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ReportExecutionProblemType {
    CoverageProblem {
        addresses: BTreeSet<u16>,
    },
    SerializedExitMismatch {
        normal_exit: VmExitReason,
        serialized_exit: VmExitReason,
    },
    SerializedStateMismatch {
        normal_exit: VmExitReason,
        serialized_exit: VmExitReason,
        normal_state: VmState,
        serialized_state: VmState,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReportExecutionProblem {
    pub event: ReportExecutionProblemType,
    pub sample: Vec<u8>,
    pub serialized: Vec<u8>,
}

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
pub enum OtaD2CUnreliable {
    Ack(u64),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum OtaD2CTransport {
    Alive {
        iteration: u64,
    },
    LastRunBlacklisted {
        address: Option<u16>,
    },
    BlacklistedAddresses {
        addresses: Vec<u16>,
    },
    ExecutionEvent(ReportExecutionProblem),
    FinishedGeneticFuzzing,
    Capabilities {
        coverage_collection: bool,
        manufacturer: String,
        processor_version_eax: u32, // cpuid 1:eax
        processor_version_ebx: u32, // cpuid 1:ebx
        processor_version_ecx: u32, // cpuid 1:ecx
        processor_version_edx: u32, // cpuid 1:edx
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum OtaC2DUnreliable {
    NOP,
    Ack(u64),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum OtaC2DTransport {
    AreYouThere,
    GetCapabilities,
    Blacklist {
        address: Vec<u16>,
    },
    DidYouExcludeAnAddressLastRun,
    GiveMeYourBlacklistedAddresses,
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
}

pub const MAX_FRAGMENT_SIZE: u64 = 1200;
pub const MAX_PAYLOAD_SIZE: u64 = 4_000_000; // ~4MB

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Ota<Unreliable, Transport> {
    Unreliable(Unreliable),
    Transport {
        session: u16,
        id: u64,
        content: Transport,
    },
    ChunkedTransport {
        session: u16,
        id: u64,
        fragment: u64,
        total_fragments: u64,
        content: Vec<u8>,
    },
}

pub trait OtaPacket<A, B>: Debug {
    fn reliable_transport(&self) -> bool;
    fn to_packet(self, sequence_number: u64, session: u16) -> Ota<A, B>;
}

impl OtaPacket<OtaD2CUnreliable, OtaD2CTransport> for OtaD2CUnreliable {
    fn reliable_transport(&self) -> bool {
        false
    }

    fn to_packet(
        self,
        _sequence_number: u64,
        _session: u16,
    ) -> Ota<OtaD2CUnreliable, OtaD2CTransport> {
        Ota::Unreliable(self)
    }
}

impl OtaPacket<OtaD2CUnreliable, OtaD2CTransport> for OtaD2CTransport {
    fn reliable_transport(&self) -> bool {
        true
    }

    fn to_packet(
        self,
        sequence_number: u64,
        session: u16,
    ) -> Ota<OtaD2CUnreliable, OtaD2CTransport> {
        Ota::Transport {
            session,
            id: sequence_number,
            content: self,
        }
    }
}

impl OtaPacket<OtaC2DUnreliable, OtaC2DTransport> for OtaC2DUnreliable {
    fn reliable_transport(&self) -> bool {
        false
    }

    fn to_packet(
        self,
        _sequence_number: u64,
        _session: u16,
    ) -> Ota<OtaC2DUnreliable, OtaC2DTransport> {
        Ota::Unreliable(self)
    }
}

impl OtaPacket<OtaC2DUnreliable, OtaC2DTransport> for OtaC2DTransport {
    fn reliable_transport(&self) -> bool {
        true
    }

    fn to_packet(
        self,
        sequence_number: u64,
        session: u16,
    ) -> Ota<OtaC2DUnreliable, OtaC2DTransport> {
        Ota::Transport {
            session,
            id: sequence_number,
            content: self,
        }
    }
}

pub type OtaD2C = Ota<OtaD2CUnreliable, OtaD2CTransport>;
pub type OtaC2D = Ota<OtaC2DUnreliable, OtaC2DTransport>;

impl OtaD2C {
    pub fn ack(&self) -> Option<OtaC2DUnreliable> {
        if let Self::Transport { id, .. } = self {
            Some(OtaC2DUnreliable::Ack(*id))
        } else {
            None
        }
    }

    pub fn serialize(&self) -> Result<Vec<u8>, String> {
        postcard::to_allocvec(self).map_err(|e| format!("Failed to serialize OTA packet: {:?}", e))
    }

    pub fn deserialize(data: &[u8]) -> Result<Self, String> {
        postcard::from_bytes(data).map_err(|e| format!("Failed to deserialize OTA packet: {:?}", e))
    }
}

impl OtaC2D {
    pub fn ack(&self) -> Option<OtaD2CUnreliable> {
        if let Self::Transport { id, .. } = self {
            Some(OtaD2CUnreliable::Ack(*id))
        } else {
            None
        }
    }

    pub fn serialize(&self) -> Result<Vec<u8>, String> {
        postcard::to_allocvec(self).map_err(|e| format!("Failed to serialize OTA packet: {:?}", e))
    }

    pub fn deserialize(data: &[u8]) -> Result<Self, String> {
        postcard::from_bytes(data).map_err(|e| format!("Failed to deserialize OTA packet: {:?}", e))
    }
}
