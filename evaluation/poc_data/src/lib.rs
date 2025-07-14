#![cfg_attr(not(feature = "clap"), no_std)]
extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use alloc::{format, vec};
use log::error;
use serde::{Deserialize, Serialize};

pub mod f0_microcode;
pub mod f1_microspectre;

pub fn agent_execute_scenario<A: for<'a> Deserialize<'a>, B: Serialize, F: Fn(A) -> B>(
    payload: &[u8],
    func: F,
) -> Vec<u8> {
    let payload = match postcard::from_bytes(payload) {
        Ok(payload) => payload,
        Err(err) => {
            error!("Failed to deserialize scenario payload: {:?}", err);
            return vec![];
        }
    };
    let result = func(payload);
    postcard::to_allocvec(&result).unwrap_or_else(|err| {
        error!("Failed to serialize poc agent result: {:?}", err);
        vec![]
    })
}

pub fn serialize<T: Serialize>(obj: &T) -> Result<Vec<u8>, String> {
    postcard::to_allocvec(obj).map_err(|err| format!("Failed to serialize: {:?}", err))
}

pub fn deserialize<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Result<T, String> {
    postcard::from_bytes(bytes).map_err(|err| format!("Failed to deserialize: {:?}", err))
}
