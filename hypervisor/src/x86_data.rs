//! x86 Data Structures Module
//!
//! This module defines x86-specific data structures used in the hypervisor,
//! particularly for task state management and hardware virtualization.


/// Task State Segment (TSS) structure
#[derive(derivative::Derivative, Default, Clone)]
#[derivative(Debug)]
#[repr(C, packed)]
pub struct TSS {
    /// Reserved field
    #[derivative(Debug = "ignore")]
    reserved_0: u32,
    /// Stack pointer entry 0
    pub rsp0: u64, // todo check if this is actually the right way around to combine lower, higher
    /// Stack pointer entry 1
    pub rsp1: u64,
    /// Stack pointer entry 2
    pub rsp2: u64,
    /// Reserved field (must be 0)
    #[derivative(Debug = "ignore")]
    reserved_1: u32,
    /// Reserved field (must be 0)
    #[derivative(Debug = "ignore")]
    reserved_2: u32,
    /// Interrupt Stack Table entry 1
    pub ist1: u64,
    /// Interrupt Stack Table entry 2
    pub ist2: u64,
    /// Interrupt Stack Table entry 3
    pub ist3: u64,
    /// Interrupt Stack Table entry 4
    pub ist4: u64,
    /// Interrupt Stack Table entry 5
    pub ist5: u64,
    /// Interrupt Stack Table entry 6
    pub ist6: u64,
    /// Interrupt Stack Table entry 7
    pub ist7: u64,
    /// Reserved field
    #[derivative(Debug = "ignore")]
    reserved_3: u32,
    /// Reserved field
    #[derivative(Debug = "ignore")]
    reserved_4: u32,
    /// Reserved field
    #[derivative(Debug = "ignore")]
    reserved_5: u16,
    /// I/O Map Base Address
    pub iomap_base: u16,
}

// Verify the TSS structure size matches the x86 specification
const _: () = assert!(core::mem::size_of::<TSS>() == 104);
