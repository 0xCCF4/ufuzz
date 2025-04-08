#![no_std]

use crate::genetic_pool::GeneticSampleRating;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Debug;
use hypervisor::state::{VmExitReason, VmState};
use performance_timing::measurements::MeasureValues;
use serde::{Deserialize, Serialize};

extern crate alloc;

pub mod decoder;
pub mod genetic_pool;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[repr(C)]
pub enum ReportExecutionProblem {
    CoverageProblem {
        address: u16,
        coverage_exit: Option<VmExitReason>,
        coverage_state: Option<VmState>,
    },
    SerializedMismatch {
        serialized_exit: Option<VmExitReason>,
        serialized_state: Option<VmState>,
    },
    StateTraceMismatch {
        index: u64,
        normal: Option<VmState>,
        serialized: Option<VmState>,
    },
    VeryLikelyBug,
}

impl ReportExecutionProblem {
    pub const MAX_PER_PACKET: usize = 3;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecutionResult {
    pub coverage: BTreeMap<u16, u16>,
    pub exit: VmExitReason,
    pub state: VmState,
    pub serialized: Option<Code>,
    pub fitness: GeneticSampleRating,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum OtaD2CUnreliable {
    Ack(u64),
    LogMessage { level: log::Level, message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OtaD2CTransport {
    LastRunBlacklisted {
        address: Option<u16>,
    },
    BlacklistedAddresses {
        addresses: Vec<u16>,
    },
    ExecutionEvents(Vec<ReportExecutionProblem>),
    ExecutionResult {
        exit: VmExitReason,
        state: VmState,
        fitness: GeneticSampleRating,
    },
    Capabilities {
        coverage_collection: bool,
        manufacturer: String,
        processor_version_eax: u32, // cpuid 1:eax
        processor_version_ebx: u32, // cpuid 1:ebx
        processor_version_ecx: u32, // cpuid 1:ecx
        processor_version_edx: u32, // cpuid 1:edx
    },
    Coverage {
        coverage: Vec<(u16, u16)>,
    },
    Serialized {
        serialized: Option<Code>,
    },
    LogMessage {
        level: log::Level,
        message: String,
    },
    PerformanceTiming {
        measurements: BTreeMap<String, MeasureValues<f64>>,
    },
    ResetSession,
}

pub type Code = Vec<u8>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum OtaC2DUnreliable {
    NOP,
    Ack(u64),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum OtaC2DTransport {
    AreYouThere,
    GetCapabilities,
    Blacklist { address: Vec<u16> },
    DidYouExcludeAnAddressLastRun,
    GiveMeYourBlacklistedAddresses,
    ReportPerformanceTiming,

    SetRandomSeed { seed: u64 },
    ExecuteSample { code: Code },

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
