//! Fuzzer Device Library
//!
//! This library implements the on device fuzzing logic that executes fuzzing inputs

#![feature(new_zeroed_alloc)]
#![allow(incomplete_features)]
#![feature(generic_const_exprs)]
#![cfg_attr(feature = "no_std", no_std)]
#![feature(stmt_expr_attributes)]
#![feature(proc_macro_hygiene)]

pub mod cmos;
pub mod controller_connection;
pub mod executor;
pub mod heuristic;
pub mod mutation_engine;
pub mod perf_monitor;

extern crate alloc;

use crate::cmos::CMOS;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Debug;
use iced_x86::{Decoder, DecoderOptions, Formatter, Instruction, NasmFormatter};

use hypervisor::state::VmState;
#[cfg(feature = "uefi")]
use uefi::{print, println};

/// Disassembles and prints x86-64 code with instruction addresses and bytes
///
/// # Arguments
///
/// * `code` - A slice of bytes containing the x86-64 machine code to disassemble
pub fn disassemble_code(code: &[u8]) {
    let mut decoder = Decoder::with_ip(64, code, 0, DecoderOptions::NONE);
    let mut formatter = NasmFormatter::new();

    formatter.options_mut().set_digit_separator("`");
    formatter.options_mut().set_first_operand_char_index(10);
    formatter.options_mut().set_show_useless_prefixes(true);

    let mut output = String::new();
    let mut instruction = Instruction::default();

    while decoder.can_decode() {
        decoder.decode_out(&mut instruction);
        output.clear();
        formatter.format(&instruction, &mut output);

        // Eg. "00007FFAC46ACDB2 488DAC2400FFFFFF     lea       rbp,[rsp-100h]"
        print!("{:016X} ", instruction.ip());
        let start_index = (instruction.ip() - 0) as usize;
        let instr_bytes = &code[start_index..start_index + instruction.len()];
        for b in instr_bytes.iter() {
            print!("{:02X}", b);
        }
        if instr_bytes.len() < 10 {
            for _ in 0..10 - instr_bytes.len() {
                print!("  ");
            }
        }
        println!(" {}", output);
    }
}

/// Persistent data structure for storing application state across executions
///
/// This structure is stored in CMOS memory and contains version information
/// and application state that persists between program runs.
#[repr(C)]
pub struct PersistentApplicationData {
    /// Version identifier for the application
    version: u32,
    /// Current state of the application
    pub state: PersistentApplicationState,
}

impl PersistentApplicationData {
    /// Returns the current application version
    ///
    /// The version is derived from the build timestamp hash.
    pub fn this_app_version() -> u32 {
        let hex_chars = env!("BUILD_TIMESTAMP_HASH").as_bytes();
        assert!(hex_chars.len() == 4 * 2);
        let mut bytes = hex_chars
            .chunks(2)
            .map(|c| u8::from_str_radix(core::str::from_utf8(c).unwrap(), 16).unwrap());
        u32::from_le_bytes([
            bytes.next().unwrap(),
            bytes.next().unwrap(),
            bytes.next().unwrap(),
            bytes.next().unwrap(),
        ])
    }
    /// Checks if the stored version matches the current application version
    pub fn is_same_program_version(&self) -> bool {
        self.version == Self::this_app_version()
    }
}

impl Default for PersistentApplicationData {
    fn default() -> Self {
        Self {
            version: Self::this_app_version(),
            state: PersistentApplicationState::Idle,
        }
    }
}

const _: () = CMOS::<PersistentApplicationData>::size_check();

/// Possible states of the persistent application
#[repr(C, u8)]
pub enum PersistentApplicationState {
    /// Application is idle
    Idle = 0,
    /// Application is collecting coverage data with specified coverage ID
    CollectingCoverage(u16) = 1,
}

/// Represents a trace of executed instructions
///
/// This structure tracks both the sequence of executed instructions and
/// their execution counts, normalized to a 1GB address space.
#[derive(Default, Clone)]
pub struct Trace {
    /// Sequence of instruction pointers in execution order
    pub sequence: Vec<u64>,
    /// Map of instruction pointers to their execution counts
    pub hit: BTreeMap<u64, u64>,
}

impl Debug for Trace {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.sequence.fmt(f)
    }
}

impl Trace {
    /// Creates a new trace from a sequence of instruction pointers
    ///
    /// Normalizes all instruction pointers to a 1GB address space and
    /// counts their occurrences.
    pub fn new(mut data: Vec<u64>) -> Self {
        data.iter_mut().for_each(|x| *x = *x % (1u64 << 30)); // map to 1GB page

        let mut hit = BTreeMap::new();
        for &ip in data.iter() {
            *hit.entry(ip).or_insert(0) += 1;
        }

        Self {
            sequence: data,
            hit,
        }
    }
    /// Checks if an instruction pointer was executed
    pub fn was_executed(&self, ip: u64) -> bool {
        self.hit.contains_key(&ip)
    }

    /// Clears the trace data
    pub fn clear(&mut self) {
        self.sequence.clear();
        self.hit.clear();
    }

    /// Adds an instruction pointer value to the trace
    ///
    /// Normalizes the instruction pointer to a 1GB address space.
    pub fn push(&mut self, ip: u64) {
        let ip = ip % (1u64 << 30);
        self.sequence.push(ip);
        *self.hit.entry(ip).or_insert(0) += 1;
    }

    /// Returns an iterator over the instruction pointer sequence
    pub fn iter(&self) -> impl Iterator<Item = &u64> {
        self.sequence.iter()
    }
}

/// Represents a trace of virtual machine states
///
/// This structure tracks the sequence of VM states during execution,
/// allowing for state comparison and analysis.
#[derive(Default, Clone)]
pub struct StateTrace<A> {
    /// Sequence of VM states in execution order
    pub state: Vec<A>,
}

impl<A> StateTrace<A>
where
    A: PartialEq,
{
    /// Creates a new state trace from a sequence of VM states
    pub fn new(state: Vec<A>) -> Self {
        Self { state }
    }

    /// Adds a VM state to the trace
    pub fn push(&mut self, state: A) {
        self.state.push(state);
    }

    /// Finds the first difference between two state traces using a custom comparison function
    fn difference<F: Fn(&A, &A) -> bool>(&self, other: &Self, equal: F) -> Option<usize> {
        for (i, state) in self.state.iter().enumerate() {
            let other = other.state.get(i);

            if other.is_none() {
                return Some(i);
            }

            if equal(state, other.unwrap()) {
                return Some(i);
            }
        }

        if self.state.len() != other.state.len() {
            return Some(self.state.len());
        }

        None
    }

    /// Finds the first difference between two state traces
    ///
    /// Returns the index of the first state that differs between the traces.
    pub fn first_difference(&self, other: &Self) -> Option<usize> {
        self.difference(other, |a, b| a == b)
    }

    /// Clears the state trace
    pub fn clear(&mut self) {
        self.state.clear();
    }

    /// Returns the number of states in the trace
    pub fn len(&self) -> usize {
        self.state.len()
    }
}

impl StateTrace<VmState> {
    /// Finds the first difference between two state traces ignoring addresses
    ///
    /// Returns the index of the first state that differs between the traces,
    /// comparing states without considering memory addresses.
    pub fn first_difference_no_addresses(&self, other: &Self) -> Option<usize> {
        self.difference(other, |a, b| a.is_equal_no_address_compare(b))
    }

    /// Gets a VM state at a specific index
    pub fn get(&self, index: usize) -> Option<&VmState> {
        self.state.get(index)
    }
}
