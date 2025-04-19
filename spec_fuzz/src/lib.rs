#![feature(new_zeroed_alloc)]
#![allow(incomplete_features)]
#![feature(generic_const_exprs)]
#![feature(stmt_expr_attributes)]
#![feature(proc_macro_hygiene)]
#![no_std]

pub mod controller_connection;
pub mod patches;

extern crate alloc;

use crate::controller_connection::ControllerConnection;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use log::Level;
use ucode_compiler_dynamic::instruction::Instruction;
use ucode_compiler_dynamic::sequence_word::SequenceWord;
use x86_perf_counter::PerfEventSpecifier;

pub fn check_if_pmc_stable(
    udp: &mut ControllerConnection,
    _perf_counter_setup: Vec<PerfEventSpecifier>,
) -> BTreeMap<u8, bool> {
    let _ = udp.log_reliable(Level::Trace, "check pmc stable");

    BTreeMap::new()
}

pub struct SpeculationResult {
    pub arch_reg_difference: BTreeMap<String, (u64, u64)>,
    pub perf_counter_difference: BTreeMap<u8, (u64, u64)>,
}

pub fn execute_speculation(
    udp: &mut ControllerConnection,
    _triad: [Instruction; 3],
    _sequence_word: SequenceWord,
    _perf_counter_setup: Vec<PerfEventSpecifier>,
) -> SpeculationResult {
    let _ = udp.log_reliable(Level::Trace, "execute speculation");

    SpeculationResult {
        arch_reg_difference: BTreeMap::new(),
        perf_counter_difference: BTreeMap::new(),
    }
}
