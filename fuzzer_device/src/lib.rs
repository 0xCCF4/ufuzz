#![feature(new_zeroed_alloc)]
#![allow(incomplete_features)]
#![feature(generic_const_exprs)]
#![cfg_attr(feature = "no_std", no_std)]

pub mod cmos;
pub mod controller_connection;
pub mod executor;
pub mod heuristic;
pub mod mutation_engine;

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

#[repr(C)]
pub struct PersistentApplicationData {
    version: u32,
    pub state: PersistentApplicationState,
}

impl PersistentApplicationData {
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

#[repr(C, u8)]
pub enum PersistentApplicationState {
    Idle = 0,
    CollectingCoverage(u16) = 1,
}

#[derive(Default, Clone)]
pub struct Trace {
    pub sequence: Vec<u64>,
    pub hit: BTreeMap<u64, u64>,
}

impl Debug for Trace {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.sequence.fmt(f)
    }
}

impl Trace {
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
    pub fn was_executed(&self, ip: u64) -> bool {
        self.hit.contains_key(&ip)
    }

    pub fn clear(&mut self) {
        self.sequence.clear();
        self.hit.clear();
    }

    pub fn push(&mut self, ip: u64) {
        let ip = ip % (1u64 << 30);
        self.sequence.push(ip);
        *self.hit.entry(ip).or_insert(0) += 1;
    }

    pub fn iter(&self) -> impl Iterator<Item = &u64> {
        self.sequence.iter()
    }
}

#[derive(Default, Clone)]
pub struct StateTrace {
    pub state: Vec<VmState>,
}

impl StateTrace {
    pub fn new(state: Vec<VmState>) -> Self {
        Self { state }
    }

    pub fn push(&mut self, state: VmState) {
        self.state.push(state);
    }

    fn difference<A: Fn(&VmState, &VmState) -> bool>(
        &self,
        other: &Self,
        equal: A,
    ) -> Option<usize> {
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

    pub fn first_difference(&self, other: &Self) -> Option<usize> {
        self.difference(other, |a, b| a == b)
    }

    pub fn first_difference_no_addresses(&self, other: &Self) -> Option<usize> {
        self.difference(other, |a, b| a.is_equal_no_address_compare(b))
    }

    pub fn get(&self, index: usize) -> Option<&VmState> {
        self.state.get(index)
    }

    pub fn clear(&mut self) {
        self.state.clear();
    }

    pub fn to_trace(&self, trace: &mut Trace) {
        trace.clear();
        for state in self.state.iter() {
            trace.push(state.standard_registers.rip);
        }
    }

    pub fn trace_vec(&self) -> Vec<u64> {
        let mut trace = Vec::with_capacity(self.state.len());
        for state in self.state.iter() {
            trace.push(state.standard_registers.rip);
        }
        trace
    }

    pub fn len(&self) -> usize {
        self.state.len()
    }
}
