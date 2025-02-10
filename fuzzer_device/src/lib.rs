#![feature(new_zeroed_alloc)]
#![allow(incomplete_features)]
#![feature(generic_const_exprs)]
#![cfg_attr(feature = "no_std", no_std)]

pub mod cmos;
pub mod executor;
pub mod genetic_breeding;
pub mod heuristic;
pub mod mutation_engine;

extern crate alloc;

use crate::cmos::CMOS;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Debug;
use iced_x86::{Decoder, DecoderOptions, Formatter, Instruction, NasmFormatter};

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
    pub data: Vec<u64>,
    pub hit: BTreeMap<u64, u64>,
}

impl Debug for Trace {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.data.fmt(f)
    }
}

impl Trace {
    pub fn new(mut data: Vec<u64>) -> Self {
        data.iter_mut().for_each(|x| *x = *x % (1u64 << 30)); // map to 1GB page

        let mut hit = BTreeMap::new();
        for &ip in data.iter() {
            *hit.entry(ip).or_insert(0) += 1;
        }

        Self { data, hit }
    }
    pub fn was_executed(&self, ip: u64) -> bool {
        self.hit.contains_key(&ip)
    }

    pub fn clear(&mut self) {
        self.data.clear();
        self.hit.clear();
    }

    pub fn push(&mut self, ip: u64) {
        let ip = ip % (1u64 << 30);
        self.data.push(ip);
        *self.hit.entry(ip).or_insert(0) += 1;
    }

    pub fn iter(&self) -> impl Iterator<Item = &u64> {
        self.data.iter()
    }
}
