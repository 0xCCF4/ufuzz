#![no_std]

use x86_perf_counter::PerfEventSpecifier;

pub mod patches;

/// This event counts the number of uops delivered to Instruction Decode Queue (IDQ)
/// from the MITE path. Counting includes uops that may "bypass" the IDQ. This also
/// means that uops are not being delivered from the Decode Stream Buffer (DSB).
pub const IDQ_MITE_UOPS: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0x79,
    umask: 0x04,
    edge_detect: None,
    cmask: None,
};

/// This event counts cycles during which uops are being delivered to Instruction Decode
/// Queue (IDQ) from the MITE path. Counting includes uops that may "bypass" the IDQ.
pub const IDQ_MITE_CYCLES: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0x79,
    umask: 0x04,
    edge_detect: None,
    cmask: Some(1),
};

/// This event counts the number of uops delivered to Instruction Decode Queue (IDQ)
/// from the Decode Stream Buffer (DSB) path. Counting includes uops that may "bypass" the IDQ.
pub const IDQ_DSB_UOPS: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0x79,
    umask: 0x08,
    edge_detect: None,
    cmask: None,
};

/// This event counts cycles during which uops are being delivered to Instruction Decode
/// Queue (IDQ) from the Decode Stream Buffer (DSB) path. Counting includes uops that
/// may "bypass" the IDQ.
pub const IDQ_DSB_CYCLES: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0x79,
    umask: 0x08,
    edge_detect: None,
    cmask: Some(1),
};

/// This event counts the number of uops initiated by Decode Stream Buffer (DSB) that
/// are being delivered to Instruction Decode Queue (IDQ) while the Microcode Sequencer
/// (MS) is busy. Counting includes uops that may "bypass" the IDQ.
pub const IDQ_MS_DSB_UOPS: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0x79,
    umask: 0x10,
    edge_detect: None,
    cmask: None,
};

/// This event counts cycles during which uops initiated by Decode Stream Buffer (DSB)
/// are being delivered to Instruction Decode Queue (IDQ) while the Microcode Sequencer
/// (MS) is busy. Counting includes uops that may "bypass" the IDQ.
pub const IDQ_MS_DSB_CYCLES: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0x79,
    umask: 0x10,
    edge_detect: None,
    cmask: Some(1),
};

/// This event counts the number of deliveries to Instruction Decode Queue (IDQ)
/// initiated by Decode Stream Buffer (DSB) while the Microcode Sequencer (MS) is busy.
/// Counting includes uops that may "bypass" the IDQ.
pub const IDQ_MS_DSB_OCCUR: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0x79,
    umask: 0x10,
    edge_detect: Some(1),
    cmask: Some(1),
};

/// This event counts the number of cycles 4 uops were delivered to Instruction Decode
/// Queue (IDQ) from the Decode Stream Buffer (DSB) path. Counting includes uops that
/// may "bypass" the IDQ.
pub const IDQ_ALL_DSB_CYCLES_4_UOPS: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0x79,
    umask: 0x18,
    edge_detect: None,
    cmask: Some(4),
};

/// This event counts the number of cycles uops were delivered to Instruction Decode
/// Queue (IDQ) from the Decode Stream Buffer (DSB) path. Counting includes uops that
/// may "bypass" the IDQ.
pub const IDQ_ALL_DSB_CYCLES_ANY_UOPS: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0x79,
    umask: 0x18,
    edge_detect: None,
    cmask: Some(1),
};

/// This event counts the number of uops initiated by MITE and delivered to Instruction
/// Decode Queue (IDQ) while the Microcode Sequencer (MS) is busy. Counting includes
/// uops that may "bypass" the IDQ.
pub const IDQ_MS_MITE_UOPS: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0x79,
    umask: 0x20,
    edge_detect: None,
    cmask: None,
};

/// This event counts the number of cycles 4 uops were delivered to Instruction Decode
/// Queue (IDQ) from the MITE path. Counting includes uops that may "bypass" the IDQ.
/// This also means that uops are not being delivered from the Decode Stream Buffer (DSB).
pub const IDQ_ALL_MITE_CYCLES_4_UOPS: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0x79,
    umask: 0x24,
    edge_detect: None,
    cmask: Some(4),
};

/// This event counts the number of cycles uops were delivered to Instruction Decode
/// Queue (IDQ) from the MITE path. Counting includes uops that may "bypass" the IDQ.
/// This also means that uops are not being delivered from the Decode Stream Buffer (DSB).
pub const IDQ_ALL_MITE_CYCLES_ANY_UOPS: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0x79,
    umask: 0x24,
    edge_detect: None,
    cmask: Some(1),
};

/// This event counts the total number of uops delivered to Instruction Decode Queue
/// (IDQ) while the Microcode Sequencer (MS) is busy. Counting includes uops that may
/// "bypass" the IDQ. Uops may be initiated by Decode Stream Buffer (DSB) or MITE.
pub const IDQ_MS_UOPS: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0x79,
    umask: 0x30,
    edge_detect: None,
    cmask: None,
};

/// This event counts cycles during which uops are being delivered to Instruction Decode
/// Queue (IDQ) while the Microcode Sequencer (MS) is busy. Counting includes uops that
/// may "bypass" the IDQ. Uops may be initiated by Decode Stream Buffer (DSB) or MITE.
pub const IDQ_MS_CYCLES: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0x79,
    umask: 0x30,
    edge_detect: None,
    cmask: Some(1),
};

/// Number of switches from DSB (Decode Stream Buffer) or MITE (legacy decode pipeline)
/// to the Microcode Sequencer.
pub const IDQ_MS_SWITCHES: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0x79,
    umask: 0x30,
    edge_detect: Some(1),
    cmask: Some(1),
};

/// This event counts the number of uops delivered to Instruction Decode Queue (IDQ)
/// from the MITE path. Counting includes uops that may "bypass" the IDQ. This also
/// means that uops are not being delivered from the Decode Stream Buffer (DSB).
pub const IDQ_MITE_ALL_UOPS: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0x79,
    umask: 0x3C,
    edge_detect: None,
    cmask: None,
};

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

/// This event counts when the last uop of a branch instruction retires, which
/// corrected a misprediction of the branch prediction hardware at execution time.
pub const BRANCH_MISSES_RETIRED: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0xC5,
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

/// This event counts uops which retired. It is a precise event.
pub const UOPS_RETIRED_ANY: PerfEventSpecifier = PerfEventSpecifier {
    event_select: 0xC2,
    umask: 0x00,
    edge_detect: None,
    cmask: None,
};
