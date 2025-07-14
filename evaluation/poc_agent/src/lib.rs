#![feature(adt_const_params)]
#![no_std]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
use log::error;
use poc_data::agent_execute_scenario;

pub mod f0_microcode;
pub mod f1_microspectre;

pub fn execute(scenario: &str, payload: &[u8]) -> Vec<u8> {
    match scenario {
        poc_data::f0_microcode::NAME => agent_execute_scenario(payload, f0_microcode::execute),
        poc_data::f1_microspectre::NAME => {
            agent_execute_scenario(payload, f1_microspectre::execute)
        }
        _ => {
            error!("Unknown scenario: {}", scenario);
            vec![]
        }
    }
}
