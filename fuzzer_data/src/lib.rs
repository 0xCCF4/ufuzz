//! # Fuzzer Data Types
//!
//! This crate provides shared data types, used by both the fuzzer agent and the fuzzer controller.
//! It includes support for:
//!
//! - Genetic algorithm-based fuzzing
//! - Instruction corpus management
//! - Over-the-air (OTA) communication protocols
//! - Execution result tracking
//! - Performance measurements
//!
//! The crate is designed to work in a no_std environment and uses serialization
//! for communication between components.
#![no_std]

use crate::genetic_pool::GeneticSampleRating;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Debug;
use hypervisor::state::{GuestRegisters, VmExitReason, VmState};
use performance_timing::measurements::MeasureValues;
use serde::{Deserialize, Serialize};
use ucode_compiler_dynamic::instruction::Instruction;
use ucode_compiler_dynamic::sequence_word::SequenceWord;
use x86_perf_counter::PerfEventSpecifier;

extern crate alloc;

pub mod decoder;
pub mod genetic_pool;
pub mod instruction_corpus;

/// Problems that can occur during execution of fuzzing operations
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[repr(C)]
pub enum ReportExecutionProblem {
    /// Problem with coverage measurement
    CoverageProblem {
        /// Address where the problem occurred
        address: u16,
        /// Exit reason from coverage measurement, if different to normal exit
        coverage_exit: Option<VmExitReason>,
        /// VM state from coverage measurement, if different to normal exit
        coverage_state: Option<VmState>,
    },
    /// Mismatch in serialized execution results
    SerializedMismatch {
        /// Exit reason from serialized execution, if different to normal exit
        serialized_exit: Option<VmExitReason>,
        /// VM state from serialized execution, if different to normal exit
        serialized_state: Option<VmState>,
    },
    /// Mismatch in state trace
    StateTraceMismatch {
        /// Index where the mismatch occurred
        index: u64,
        /// Normal execution state
        normal: Option<VmState>,
        /// Serialized execution state
        serialized: Option<VmState>,
    },
    /// High probability of a bug being found
    VeryLikelyBug,
    /// Access to coverage measurement area
    AccessCoverageArea,
}

impl ReportExecutionProblem {
    /// Maximum number of problems that can be reported in a single packet
    pub const MAX_PER_PACKET: usize = 3;
}

/// Result of executing a fuzzing operation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecutionResult {
    /// Coverage map (address -> count)
    pub coverage: BTreeMap<u16, u16>,
    /// Exit reason from execution
    pub exit: VmExitReason,
    /// Final VM state
    pub state: VmState,
    /// Serialized code (if available)
    pub serialized: Option<Code>,
    /// Genetic algorithm fitness rating
    pub fitness: GeneticSampleRating,
}

/// Memory access traced
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryAccess {
    /// Address accessed
    pub address: u64,
    /// Read
    pub read: bool,
    /// Write
    pub write: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TraceResult {
    Finished(
        /// Exit reason from execution
        VmExitReason,
    ),
    Running {
        /// Current VM state
        index: u16,
        state: VmState,
        memory_accesses: Vec<MemoryAccess>,
    },
}

impl From<VmExitReason> for TraceResult {
    fn from(exit: VmExitReason) -> Self {
        TraceResult::Finished(exit)
    }
}

/// Unreliable device-to-controller messages
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum OtaD2CUnreliable {
    /// Acknowledgment of a message
    Ack(u64),
    /// Log message with level
    LogMessage { level: log::Level, message: String },
}

/// Reliable device-to-controller transport messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OtaD2CTransport {
    /// Last run was blacklisted at an address
    LastRunBlacklisted {
        /// Address that was blacklisted
        address: Option<u16>,
    },
    /// List of blacklisted addresses
    BlacklistedAddresses {
        /// Blacklisted addresses
        addresses: Vec<u16>,
    },
    /// Execution problems encountered
    ExecutionEvents(Vec<ReportExecutionProblem>),
    /// Result of execution
    ExecutionResult {
        /// Exit reason
        exit: VmExitReason,
        /// Final VM state
        state: VmState,
        /// Genetic algorithm fitness
        fitness: GeneticSampleRating,
    },
    /// Device capabilities
    Capabilities {
        /// Whether coverage collection is supported
        coverage_collection: bool,
        /// CPU manufacturer string
        manufacturer: String,
        /// Number of performance counters
        pmc_number: u8,
        /// CPUID 1:EAX (version information)
        processor_version_eax: u32,
        /// CPUID 1:EBX (additional info)
        processor_version_ebx: u32,
        /// CPUID 1:ECX (feature flags)
        processor_version_ecx: u32,
        /// CPUID 1:EDX (feature flags)
        processor_version_edx: u32,
    },
    /// Coverage information
    Coverage {
        /// Coverage map entries
        coverage: Vec<(u16, u16)>,
    },
    /// Serialized code
    Serialized {
        /// Serialized code (if available)
        serialized: Option<Code>,
    },
    /// Log message
    LogMessage {
        /// Log level
        level: log::Level,
        /// Message content
        message: String,
    },
    /// Performance timing measurements
    PerformanceTiming {
        /// Map of measurement name to values
        measurements: BTreeMap<String, MeasureValues<f64>>,
    },
    /// Request to reset session
    ResetSession,
    /// Results of PMC stability check
    PMCStableCheckResults {
        /// Vector of stability flags
        pmc_stable: Vec<bool>,
    },
    /// Result of microcode speculation test
    UCodeSpeculationResult(SpeculationResult),
    /// Tracing result
    TraceResult(TraceResult),
}

/// Result of a speculation test
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpeculationResult {
    /// Register state before speculation
    pub arch_before: GuestRegisters,
    /// Register state after speculation
    pub arch_after: GuestRegisters,
    /// Performance counter values
    pub perf_counters: Vec<u64>,
}

/// Type alias for code bytes
pub type Code = Vec<u8>;

/// Unreliable controller-to-device messages
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum OtaC2DUnreliable {
    /// No operation
    NOP,
    /// Acknowledgment of a message
    Ack(u64),
}

/// Reliable controller-to-device transport messages
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum OtaC2DTransport {
    /// Check if device is responsive
    AreYouThere,
    /// Request device capabilities
    GetCapabilities,
    /// Blacklist addresses
    Blacklist {
        /// Addresses to blacklist
        address: Vec<u16>,
    },
    /// Query if address was excluded in last run
    DidYouExcludeAnAddressLastRun,
    /// Request list of blacklisted addresses
    GiveMeYourBlacklistedAddresses,
    /// Request performance timing report
    ReportPerformanceTiming,
    /// Set random number generator seed
    SetRandomSeed {
        /// Random seed value
        seed: u64,
    },
    /// Execute a code sample
    ExecuteSample {
        /// Code to execute
        code: Code,
    },
    /// Trace a code sample
    TraceSample {
        /// Code to trace
        code: Code,
        /// Maximum number of instructions to trace
        max_iterations: u64,
        /// Whether to record memory accesses
        record_memory_access: bool,
    },
    /// Test microcode speculation
    UCodeSpeculation {
        /// Triad to test
        triad: [Instruction; 3],
        /// Sequence word for the triad
        sequence_word: SequenceWord,
        /// Performance counter configuration
        perf_counter_setup: Vec<PerfEventSpecifier>,
    },
    /// Test PMC stability
    TestIfPMCStable {
        /// Performance counter configuration
        perf_counter_setup: Vec<PerfEventSpecifier>,
    },
    /// Request device reboot
    Reboot,
}

/// Maximum size of a message fragment
pub const MAX_FRAGMENT_SIZE: u64 = 1200;
/// Maximum size of a complete payload
pub const MAX_PAYLOAD_SIZE: u64 = 4_000_000; // ~4MB

/// Over-the-air message container
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Ota<Unreliable, Transport> {
    /// Unreliable message
    Unreliable(Unreliable),
    /// Reliable transport message
    Transport {
        /// Session identifier
        session: u16,
        /// Message identifier
        id: u64,
        /// Message content
        content: Transport,
    },
    /// Chunked transport message
    ChunkedTransport {
        /// Session identifier
        session: u16,
        /// Message identifier
        id: u64,
        /// Fragment number
        fragment: u64,
        /// Total number of fragments
        total_fragments: u64,
        /// Fragment content
        content: Vec<u8>,
    },
}

/// Trait for OTA packet types
pub trait OtaPacket<A, B>: Debug {
    /// Whether the packet requires reliable transport
    fn reliable_transport(&self) -> bool;
    /// Convert to an OTA packet
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
