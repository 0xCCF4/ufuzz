#![no_std]

extern crate core;

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq, Ord, PartialOrd)]
pub struct PerfEventSpecifier {
    pub event_select: u8,
    pub umask: u8,
    pub edge_detect: Option<u8>,
    pub cmask: Option<u8>,
}

impl AsRef<PerfEventSpecifier> for PerfEventSpecifier {
    fn as_ref(&self) -> &PerfEventSpecifier {
        &self
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
mod x86;

use serde::{Deserialize, Serialize};
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
pub use x86::*;

/// This event counts the number of times the Microcode Sequencer (MS) starts
/// a flow of uops from the MSROM. It does not count every time a uop is read
/// from the MSROM. The most common case that this counts is when a micro-coded
/// instruction is encountered by the front end of the machine. Other cases
/// include when an instruction encounters a fault, trap, or microcode assist
/// of any sort that initiates a flow of uops. The event will count MS startups
/// for uops that are speculative, and subsequently cleared by branch mispredict
/// or a machine clear.
pub const MS_DECODED_MS_ENTRY: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0xE7,
    umask: 0x01,
    edge_detect: None,
    cmask: None,
};

/// This event counts uops issued by the front end and allocated into the back
/// end of the machine. This event counts uops that retire as well as uops that
/// were speculatively executed but didn't retire. The sort of speculative uops
/// that might be counted includes, but is not limited to those uops issued in
/// the shadow of a miss-predicted branch, those uops that are inserted during
/// an assist (such as for a denormal floating point result), and (previously
/// allocated) uops that might be canceled during a machine clear.
pub const UOPS_ISSUED_ANY: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0x0E,
    umask: 0x00,
    edge_detect: None,
    cmask: None,
};

/// This event counts when the last uop of an instruction retires.
pub const INSTRUCTIONS_RETIRED: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0xC0,
    umask: 0x00,
    edge_detect: None,
    cmask: None,
};

/// This event counts uops which retired. It is a precise event.
pub const UOPS_RETIRED_ANY: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0xC2,
    umask: 0x00,
    edge_detect: None,
    cmask: None,
};
