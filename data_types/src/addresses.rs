//! Types for representing and manipulating microcode addresses
//! 
//! This module provides types for working with various kinds of addresses used in
//! microcode operations:
//! 
//! - Linear addresses: Simple sequential addresses
//! - Instruction addresses: Addresses in the microcode instruction space
//! - MSRAM addresses: Addresses in the microcode RAM
//! - MSROM addresses: Addresses in the microcode ROM
//! - Hook addresses: Addresses for microcode hooks
//! 
//! Each address type implements common traits for arithmetic operations and
//! conversions between different address spaces.

use crate::cstd::fmt;
use core::ops::{Add, Div, Mul, Sub};

/// Common trait for all address types
pub trait Address:
    Clone + Copy + PartialEq + Eq + PartialOrd + Ord + fmt::Debug + fmt::Display
{
    /// Get the raw value of the address.
    /// In general, it is considered bad practice to
    /// do any arithmetic with the raw value.
    fn address(&self) -> usize;
}

/// Trait for addresses in the MSRAM (Microcode RAM)
pub trait MSRAMAddress: Address {}

/// A linear address, starting at 0 and counting up by 1 for each unit.
/// 
/// This is the simplest form of address, used as a base for other address types.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LinearAddress(usize);

impl Address for LinearAddress {
    fn address(&self) -> usize {
        self.0
    }
}

impl LinearAddress {
    /// Creates a new linear address from a constant value
    #[track_caller]
    pub const fn from_const(value: usize) -> Self {
        LinearAddress(value)
    }

    /// The zero address (0x0000)
    pub const ZERO: LinearAddress = LinearAddress::from_const(0);
    /// The minimum valid address (0x0000)
    pub const MIN: LinearAddress = LinearAddress::ZERO;
}

impl From<usize> for LinearAddress {
    #[track_caller]
    fn from(value: usize) -> Self {
        LinearAddress::from_const(value)
    }
}

impl From<LinearAddress> for usize {
    #[track_caller]
    fn from(value: LinearAddress) -> Self {
        value.0
    }
}

impl fmt::Display for LinearAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "L{:04x}", self.0)
    }
}

impl fmt::Debug for LinearAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "L{:04x}", self.0)
    }
}

impl Add<usize> for LinearAddress {
    type Output = Self;

    #[track_caller]
    fn add(self, other: usize) -> Self {
        LinearAddress::from_const(self.0 + other)
    }
}

impl Sub<usize> for LinearAddress {
    type Output = Self;

    #[track_caller]
    fn sub(self, other: usize) -> Self {
        LinearAddress::from(self.0 - other)
    }
}

impl Mul<usize> for LinearAddress {
    type Output = Self;

    #[track_caller]
    fn mul(self, other: usize) -> Self {
        LinearAddress::from_const(self.0 * other)
    }
}

impl Div<usize> for LinearAddress {
    type Output = Self;
    #[track_caller]
    fn div(self, other: usize) -> Self {
        LinearAddress::from_const(self.0 / other)
    }
}

impl From<&LinearAddress> for LinearAddress {
    fn from(value: &LinearAddress) -> Self {
        *value
    }
}

/// An address in the microcode instruction space
/// 
/// - Microcode instruction RAM starts at 0x7c00
/// - Microcode ROM starts at 0x0000
/// - Each instruction is 1 unit apart
/// - Every fourth address (where addr % 4 == 3) is skipped
#[derive(Clone, Copy, Eq, Hash, Ord)]
pub struct UCInstructionAddress(usize);

impl Address for UCInstructionAddress {
    fn address(&self) -> usize {
        self.0
    }
}

impl UCInstructionAddress {
    /// Creates a new instruction address from a constant value
    /// 
    /// # Panics
    /// 
    /// Panics if the address is out of bounds (>= 0x7c00 + 4 * 128)
    #[track_caller]
    pub const fn from_const(value: usize) -> Self {
        if value >= 0x7c00 + 4 * 128 {
            panic!("Address out of bounds exception. Address is larger than addressing region [0;7DFF].")
        }

        UCInstructionAddress(value)
    }

    /// Converts the address to a constant value
    /// Only use this function for specifying constants
    pub const fn to_const(self) -> usize {
        self.0
    }

    /// Returns the instruction address for a given offset
    /// 
    /// Offsets are [0,1,2,3,...] mapping to addresses self+[0,1,2, 4,5,6, 8,9,a]
    /// 
    /// # Panics
    /// 
    /// Panics if the origin address is not a multiple of 4
    #[track_caller]
    pub fn patch_offset(&self, offset: usize) -> UCInstructionAddress {
        if (self.0 & 3) > 0 {
            panic!("Origin address must be a multiple of 4. {offset:04x} is not %mod4=0")
        }

        let addr = self.0;
        let base = offset / 3;
        let offset_in_triplet = offset % 3;

        UCInstructionAddress::from_const(addr + base * 4 + offset_in_triplet)
    }

    /// Returns the next valid instruction address
    /// 
    /// Sequence: [7c00, 7c01, 7c02, 7c04, 7c05, ...]
    /// Skips each fourth address (where addr % 4 == 3)
    pub fn next_address(&self) -> Self {
        let addr = self.0 + 1;
        UCInstructionAddress::from_const(if (addr & 3) == 3 { addr + 1 } else { addr })
    }

    /// Returns the next even address
    pub fn next_even_address(&self) -> Self {
        if self.is_even() {
            self.add(2)
        } else {
            self.next_address()
        }
    }

    /// Returns the address aligned to an even boundary
    pub fn align_even(&self) -> Self {
        Self::from_const(self.0 & !1)
    }

    /// Checks if the address can be used as a hook target
    pub fn hookable(&self) -> bool {
        self.0 % 2 == 0 && self.0 < 0x7c00
    }

    /// Returns the base address of the triad containing this address
    pub const fn triad_base(&self) -> UCInstructionAddress {
        UCInstructionAddress::from_const(self.0 & !3)
    }

    /// Checks if the address is even
    pub const fn is_even(&self) -> bool {
        self.0 % 2 == 0
    }

    /// Checks if the address is odd
    pub const fn is_odd(&self) -> bool {
        self.0 % 2 == 1
    }

    /// Returns the offset within the triad (0-3)
    pub const fn triad_offset(&self) -> u8 {
        (self.0 & 3) as u8
    }

    /// Returns a new address with the given triad offset
    pub const fn with_triad_offset(&self, offset: u8) -> Self {
        UCInstructionAddress::from_const((self.0 & !3) | offset as usize)
    }

    /// Checks if the address has the given triad offset
    pub const fn is_offset_by(&self, offset: u8) -> bool {
        self.triad_offset() == offset
    }

    /// The zero address (0x0000)
    pub const ZERO: UCInstructionAddress = UCInstructionAddress::from_const(0);
    /// The minimum valid address (0x0000)
    pub const MIN: UCInstructionAddress = UCInstructionAddress::ZERO;
    /// The start of microcode RAM (0x7c00)
    pub const MSRAM_START: UCInstructionAddress = UCInstructionAddress::from_const(0x7c00);
    /// The maximum valid address (0x7c00 + 4 * 128 - 1)
    pub const MAX: UCInstructionAddress = UCInstructionAddress::from_const(0x7c00 + 4 * 128 - 1);
}

impl fmt::Display for UCInstructionAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "U{:04x}", self.0)
    }
}

impl fmt::Debug for UCInstructionAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "U{:04x}", self.0)
    }
}

impl From<LinearAddress> for UCInstructionAddress {
    #[track_caller]
    fn from(value: LinearAddress) -> Self {
        UCInstructionAddress::from_const(value.0)
    }
}
impl From<usize> for UCInstructionAddress {
    #[track_caller]
    fn from(value: usize) -> Self {
        UCInstructionAddress::from_const(value)
    }
}
impl From<UCInstructionAddress> for usize {
    #[track_caller]
    fn from(value: UCInstructionAddress) -> Self {
        value.0
    }
}
impl From<UCInstructionAddress> for LinearAddress {
    #[track_caller]
    fn from(value: UCInstructionAddress) -> Self {
        LinearAddress::from_const(value.0)
    }
}
impl Add<usize> for UCInstructionAddress {
    type Output = Self;
    #[track_caller]
    fn add(self, other: usize) -> Self {
        UCInstructionAddress::from_const(self.0 + other)
    }
}
impl Sub<usize> for UCInstructionAddress {
    type Output = Self;
    #[track_caller]
    fn sub(self, other: usize) -> Self {
        UCInstructionAddress::from_const(self.0 - other)
    }
}

impl Add<UCInstructionAddress> for UCInstructionAddress {
    type Output = Self;
    #[track_caller]
    fn add(self, other: UCInstructionAddress) -> Self {
        UCInstructionAddress::from_const(self.0 + other.0)
    }
}
impl Sub<UCInstructionAddress> for UCInstructionAddress {
    type Output = Self;
    #[track_caller]
    fn sub(self, other: UCInstructionAddress) -> Self {
        UCInstructionAddress::from_const(self.0 - other.0)
    }
}

impl From<&UCInstructionAddress> for UCInstructionAddress {
    fn from(value: &UCInstructionAddress) -> Self {
        *value
    }
}

impl AsRef<UCInstructionAddress> for UCInstructionAddress {
    fn as_ref(&self) -> &UCInstructionAddress {
        self
    }
}

impl<T: Into<usize> + Copy> PartialEq<T> for UCInstructionAddress {
    fn eq(&self, other: &T) -> bool {
        self.0 == (*other).into()
    }
}

impl<T: Into<usize> + Copy> PartialOrd<T> for UCInstructionAddress {
    fn partial_cmp(&self, other: &T) -> Option<core::cmp::Ordering> {
        self.0.partial_cmp(&(*other).into())
    }
}

/// An address for writing to the instruction part of MSRAM
///
/// Memory layout of the MSRAM is like this:
/// [0, 1, ..., 80, 81, ..., 100, 101, ...]
/// which maps to the following ucode addresses
/// [7C00, 7C04, ..., 7C01, 7C05, ..., 7C02, 7C06, ...]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MSRAMInstructionPartWriteAddress(usize);

impl Address for MSRAMInstructionPartWriteAddress {
    fn address(&self) -> usize {
        self.0
    }
}

impl MSRAMInstructionPartWriteAddress {
    /// Creates a new write address from a constant value
    /// 
    /// # Panics
    /// 
    /// Panics if the address is out of bounds (>= 128 * 3)
    #[track_caller]
    pub const fn from_const(value: usize) -> Self {
        if value >= 128 * 4 {
            panic!("Address out of bounds. Address value exceeds size limit of 1FF.")
        }
        if (value & 3) == 3 {
            panic!("Invalid memory location. Address value has a LSB suffix of b11.")
        }
        MSRAMInstructionPartWriteAddress(value)
    }

    /// The zero address (0x0000)
    pub const ZERO: MSRAMInstructionPartWriteAddress =
        MSRAMInstructionPartWriteAddress::from_const(0);
    /// The minimum valid address (0x0000)
    pub const MIN: MSRAMInstructionPartWriteAddress = MSRAMInstructionPartWriteAddress::ZERO;
    /// The maximum valid address (128 * 3 - 1)
    pub const MAX: MSRAMInstructionPartWriteAddress =
        MSRAMInstructionPartWriteAddress::from_const(128 * 4 - 2);
}

impl MSRAMAddress for MSRAMInstructionPartWriteAddress {}

impl fmt::Display for MSRAMInstructionPartWriteAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "I{:04x}", self.0)
    }
}

impl fmt::Debug for MSRAMInstructionPartWriteAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "I{:04x}", self.0)
    }
}
impl From<LinearAddress> for MSRAMInstructionPartWriteAddress {
    #[track_caller]
    fn from(value: LinearAddress) -> Self {
        MSRAMInstructionPartWriteAddress::from_const(value.0)
    }
}
impl From<UCInstructionAddress> for MSRAMInstructionPartWriteAddress {
    #[track_caller]
    fn from(value: UCInstructionAddress) -> Self {
        let addr = value.0;
        if addr < 0x7c00 {
            panic!(
                "Address is not in the ucode RAM: {}. RAM starts at 0x7c00.",
                addr
            );
        }
        let addr = addr - 0x7c00;

        MSRAMInstructionPartWriteAddress::from_const(addr)
    }
}
impl From<MSRAMInstructionPartWriteAddress> for LinearAddress {
    #[track_caller]
    fn from(value: MSRAMInstructionPartWriteAddress) -> Self {
        LinearAddress::from_const(value.0)
    }
}
impl From<MSRAMInstructionPartWriteAddress> for UCInstructionAddress {
    #[track_caller]
    fn from(value: MSRAMInstructionPartWriteAddress) -> Self {
        let addr = value.0;

        UCInstructionAddress::from_const(0x7c00 + addr)
    }
}

impl From<&MSRAMInstructionPartWriteAddress> for MSRAMInstructionPartWriteAddress {
    fn from(value: &MSRAMInstructionPartWriteAddress) -> Self {
        *value
    }
}

/// An address for reading from the instruction part of MSRAM
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MSRAMInstructionPartReadAddress(usize);

impl Address for MSRAMInstructionPartReadAddress {
    fn address(&self) -> usize {
        self.0
    }
}

impl MSRAMInstructionPartReadAddress {
    /// Creates a new read address from a constant value
    /// 
    /// # Panics
    /// 
    /// Panics if the address is out of bounds (>= 128 * 3)
    #[track_caller]
    pub const fn from_const(value: usize) -> Self {
        if value >= 128 * 3 {
            panic!("Memory out out bounds. Address value is larger than limit 1FF.")
        }

        MSRAMInstructionPartReadAddress(value)
    }

    /// The zero address (0x0000)
    pub const ZERO: MSRAMInstructionPartReadAddress =
        MSRAMInstructionPartReadAddress::from_const(0);
    /// The minimum valid address (0x0000)
    pub const MIN: MSRAMInstructionPartReadAddress = MSRAMInstructionPartReadAddress::ZERO;
    /// The maximum valid address (128 * 3 - 1)
    pub const MAX: MSRAMInstructionPartReadAddress =
        MSRAMInstructionPartReadAddress::from_const(128 * 3 - 1);
}

impl MSRAMAddress for MSRAMInstructionPartReadAddress {}
impl From<MSRAMInstructionPartWriteAddress> for MSRAMInstructionPartReadAddress {
    #[track_caller]
    fn from(value: MSRAMInstructionPartWriteAddress) -> Self {
        let base = value.0 / 4;
        let offset = value.0 % 4;
        MSRAMInstructionPartReadAddress(offset * 0x80 + base)
    }
}
impl From<MSRAMInstructionPartReadAddress> for MSRAMInstructionPartWriteAddress {
    #[track_caller]
    fn from(value: MSRAMInstructionPartReadAddress) -> Self {
        let base = value.0 / 0x80;
        let offset = value.0 % 0x80;
        Self::from_const(offset * 4 + base)
    }
}
impl From<UCInstructionAddress> for MSRAMInstructionPartReadAddress {
    #[track_caller]
    fn from(value: UCInstructionAddress) -> Self {
        Self::from(MSRAMInstructionPartWriteAddress::from(value))
    }
}
impl From<MSRAMInstructionPartReadAddress> for UCInstructionAddress {
    #[track_caller]
    fn from(value: MSRAMInstructionPartReadAddress) -> Self {
        Self::from(MSRAMInstructionPartWriteAddress::from(value))
    }
}
impl fmt::Display for MSRAMInstructionPartReadAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Ir{:04x}", self.0)
    }
}
impl fmt::Debug for MSRAMInstructionPartReadAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Ir{:04x}", self.0)
    }
}

impl From<&MSRAMInstructionPartReadAddress> for MSRAMInstructionPartReadAddress {
    fn from(value: &MSRAMInstructionPartReadAddress) -> Self {
        *value
    }
}

/// An address in the sequence word part of MSRAM
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MSRAMSequenceWordAddress(usize);

impl Address for MSRAMSequenceWordAddress {
    fn address(&self) -> usize {
        self.0
    }
}

impl MSRAMSequenceWordAddress {
    /// Creates a new sequence word address from a constant value
    /// 
    /// # Panics
    /// 
    /// Panics if the address is out of bounds (>= 128)
    #[track_caller]
    pub const fn from_const(value: usize) -> Self {
        if value >= 128 {
            panic!("Address out of bounds exception. Address is larger than limit 0x7F.");
        }

        MSRAMSequenceWordAddress(value)
    }

    /// The zero address (0x0000)
    pub const ZERO: MSRAMSequenceWordAddress = MSRAMSequenceWordAddress::from_const(0);
    /// The minimum valid address (0x0000)
    pub const MIN: MSRAMSequenceWordAddress = MSRAMSequenceWordAddress::ZERO;
    /// The maximum valid address (127)
    pub const MAX: MSRAMSequenceWordAddress = MSRAMSequenceWordAddress::from_const(128 - 1);
}
impl MSRAMAddress for MSRAMSequenceWordAddress {}
impl fmt::Display for MSRAMSequenceWordAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "S{:04x}", self.0)
    }
}
impl fmt::Debug for MSRAMSequenceWordAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "S{:04x}", self.0)
    }
}
impl From<UCInstructionAddress> for MSRAMSequenceWordAddress {
    #[track_caller]
    fn from(value: UCInstructionAddress) -> Self {
        MSRAMSequenceWordAddress::from_const((value.0 - 0x7c00) / 4)
    }
}
impl From<LinearAddress> for MSRAMSequenceWordAddress {
    #[track_caller]
    fn from(value: LinearAddress) -> Self {
        let addr = value.address();
        MSRAMSequenceWordAddress::from_const(addr)
    }
}
impl From<MSRAMSequenceWordAddress> for UCInstructionAddress {
    #[track_caller]
    fn from(value: MSRAMSequenceWordAddress) -> Self {
        UCInstructionAddress::from_const(0x7c00 + value.0 * 4)
    }
}
impl From<MSRAMSequenceWordAddress> for LinearAddress {
    #[track_caller]
    fn from(value: MSRAMSequenceWordAddress) -> Self {
        LinearAddress::from(value.0)
    }
}
impl Add<usize> for MSRAMSequenceWordAddress {
    type Output = MSRAMSequenceWordAddress;
    #[track_caller]
    fn add(self, rhs: usize) -> Self::Output {
        MSRAMSequenceWordAddress::from_const(self.0 + rhs)
    }
}
impl Sub<usize> for MSRAMSequenceWordAddress {
    type Output = MSRAMSequenceWordAddress;
    #[track_caller]
    fn sub(self, rhs: usize) -> Self::Output {
        MSRAMSequenceWordAddress::from_const(self.0 - rhs)
    }
}

impl From<&MSRAMSequenceWordAddress> for MSRAMSequenceWordAddress {
    fn from(value: &MSRAMSequenceWordAddress) -> Self {
        *value
    }
}

/// An index into the hook table in MSRAM
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MSRAMHookIndex(usize);

impl Address for MSRAMHookIndex {
    fn address(&self) -> usize {
        self.0 * 2
    }
}

impl MSRAMHookIndex {
    /// Creates a new hook index from a constant value
    /// 
    /// # Panics
    /// 
    /// Panics if the index is out of bounds (>= 32)
    #[track_caller]
    pub const fn from_const(value: usize) -> Self {
        if value >= 32 {
            panic!("Index out of bounds exception. Index must be smaller than 32.")
        }

        MSRAMHookIndex(value)
    }

    /// The zero index (0x0000)
    pub const ZERO: Self = MSRAMHookIndex::from_const(0);
    /// The minimum valid index (0x0000)
    pub const MIN: Self = MSRAMHookIndex::ZERO;
    /// The maximum valid index (31)
    pub const MAX: Self = MSRAMHookIndex::from_const(31);
}

impl MSRAMAddress for MSRAMHookIndex {}

impl fmt::Display for MSRAMHookIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "H{:04x}", self.0)
    }
}

impl fmt::Debug for MSRAMHookIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "H{:04x}", self.0)
    }
}
impl From<LinearAddress> for MSRAMHookIndex {
    #[track_caller]
    fn from(value: LinearAddress) -> Self {
        MSRAMHookIndex::from_const(value.address())
    }
}
impl From<MSRAMHookIndex> for LinearAddress {
    #[track_caller]
    fn from(value: MSRAMHookIndex) -> Self {
        LinearAddress::from_const(value.0)
    }
}

impl Add<usize> for MSRAMHookIndex {
    type Output = Self;

    #[track_caller]
    fn add(self, other: usize) -> Self {
        MSRAMHookIndex::from_const(self.0 + other)
    }
}

impl Sub<usize> for MSRAMHookIndex {
    type Output = Self;
    #[track_caller]
    fn sub(self, other: usize) -> Self {
        MSRAMHookIndex::from_const(self.0 - other)
    }
}

impl From<&MSRAMHookIndex> for MSRAMHookIndex {
    fn from(value: &MSRAMHookIndex) -> Self {
        *value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;

    // conversion tests
    // usize -> LA
    // LA -> usize
    // usize -> UC
    // UC -> usize
    //
    // LA -> UC
    // UC -> LA

    // LA -> InstPatch
    // InstPatch -> LA

    // UC -> InstPatch
    // InstPatch -> UC

    // LA -> SeqPatch
    // SeqPatch -> LA

    // UC -> SeqPatch
    // SeqPatch -> UC

    // LA -> Hook
    // Hook -> LA

    #[track_caller]
    fn conversion_harness<A: Address + From<B>, B: Address + From<A>>(tests: &[(A, B)]) {
        for (a, b) in tests {
            let b_from_a = B::from(*a);
            let a_from_b = A::from(*b);
            assert_eq!(
                b_from_a.address(),
                b.address(),
                "Converting A to B: {:04x} -> expected {:04x} got {:04x}",
                a.address(),
                b.address(),
                b_from_a.address()
            );
            assert_eq!(
                a_from_b.address(),
                a.address(),
                "Converting B to A: {:04x} -> expected {:04x} got {:04x}",
                b.address(),
                a.address(),
                a_from_b.address()
            );
        }
    }

    #[allow(dead_code)]
    fn ucode_addr_to_patch_addr(ucode_addr: usize) -> usize {
        // from custom processing unit
        let base = ucode_addr - 0x7c00;
        let offset = base % 4;
        let row = base / 4;
        (offset * 0x80 + row) * 4
    }

    fn ucode_addr_to_patch_seqword_addr(addr: usize) -> usize {
        let base = addr - 0x7c00;
        let seq_addr = (base % 4) * 0x80 + (base / 4);
        seq_addr % 0x80
    }

    #[test]
    fn test_convert_la_uc() {
        let tests = vec![
            (LinearAddress(0), UCInstructionAddress(0)),
            (LinearAddress(1), UCInstructionAddress(1)),
            (LinearAddress(2), UCInstructionAddress(2)),
            (LinearAddress(3), UCInstructionAddress(3)),
            (LinearAddress(4), UCInstructionAddress(4)),
        ];
        conversion_harness(&tests);
    }

    #[test]
    fn test_convert_la_inst_patch() {
        // TODO
    }

    #[test]
    fn test_convert_inst_read_write() {
        // TODO
    }

    #[test]
    fn test_convert_ua_inst_patch() {
        // TODO
    }

    #[test]
    fn test_convert_la_seqw_patch() {
        let mut tests = Vec::default();

        for i in 0..0x1f {
            let la = LinearAddress(i);
            let patch_addr = MSRAMSequenceWordAddress::from_const(
                ucode_addr_to_patch_seqword_addr(4 * i + 0x7c00),
            );
            tests.push((la, patch_addr));
        }
        conversion_harness(&tests);
    }

    #[test]
    fn test_convert_uc_seqw_patch() {
        let mut tests = Vec::default();

        for i in 0x7c00..0x7c00 + 0xFF {
            let la = UCInstructionAddress(i & !0x3);
            let patch_addr =
                MSRAMSequenceWordAddress::from_const(ucode_addr_to_patch_seqword_addr(i));
            tests.push((la, patch_addr));
        }
        conversion_harness(&tests);
    }

    #[test]
    fn test_convert_la_hook() {
        let tests = vec![
            (LinearAddress(0), MSRAMHookIndex(0)),
            (LinearAddress(1), MSRAMHookIndex(2)),
            (LinearAddress(2), MSRAMHookIndex(4)),
            (LinearAddress(3), MSRAMHookIndex(6)),
            (LinearAddress(4), MSRAMHookIndex(8)),
        ];
        conversion_harness(&tests);
    }
}
