use crate::cstd::fmt;
use core::ops::{Add, Div, Mul, Sub};

pub trait Address:
    Clone + Copy + PartialEq + Eq + PartialOrd + Ord + fmt::Debug + fmt::Display
{
    /// Get the raw value of the address.
    /// In general, it is concidered bad practice to
    /// do any arithmetic with the raw value.
    fn address(&self) -> usize;
}

// A trait for addresses in the MSRAM.
pub trait MSRAMAddress: Address {}

// A linear address. Starting at 0 counting up by 1 for each unit.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LinearAddress(usize);
impl Address for LinearAddress {
    fn address(&self) -> usize {
        self.0
    }
}
impl LinearAddress {
    #[track_caller]
    pub const fn from_const(value: usize) -> Self {
        LinearAddress(value)
    }
    pub const ZERO: LinearAddress = LinearAddress::from_const(0);
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

// An address of a code instruction.
// Ucode instruction RAM starts at 0x7c00 counting up by 1
// Ucode ROM starts at 0x000
// Internally the same as Linear Address but semantically different
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct UCInstructionAddress(usize);
impl Address for UCInstructionAddress {
    fn address(&self) -> usize {
        self.0
    }
}
impl UCInstructionAddress {
    #[track_caller]
    pub const fn from_const(value: usize) -> Self {
        if value >= 0x7c00 + 4 * 128 {
            panic!("Address out of bounds exception. Address is larger than addressing region [0;7DFF].")
        }

        UCInstructionAddress(value)
    }

    /// only use this function for specifying consts
    pub const fn to_const(self) -> usize {
        self.0
    }

    /// Returns the MSRAMInstructionPartAddress for a given offset. Offsets are [0,1,2,3,...] mapping to
    /// addresses self+[0,1,2, 4,5,6, 8,9,a]
    /// #[track_caller]
    pub fn patch_offset(&self, offset: usize) -> UCInstructionAddress {
        if (self.0 & 3) > 0 {
            panic!("Origin address must be a multiple of 4. {offset:04x} is not %mod4=0")
        }

        let addr = self.0;

        let base = offset / 3;
        let offset_in_triplet = offset % 3;

        UCInstructionAddress::from_const(addr + base * 4 + offset_in_triplet)
    }

    /// When writing patch data, can be used in an iterative fashion.
    /// Returns the next valid writing address
    /// [7c00, 7c01, 7c02, 7c04, 7c05, ...] skipping each forth address
    pub fn next_patch_address(&self) -> Self {
        let addr = self.0 + 1;
        UCInstructionAddress::from_const(if (addr & 3) == 3 { addr + 1 } else { addr })
    }

    pub fn hookable(&self) -> bool {
        self.0 % 2 == 0 && self.0 < 0x7c00 // todo check if <0x7c00 is necessary
    }

    pub const ZERO: UCInstructionAddress = UCInstructionAddress::from_const(0);
    pub const MIN: UCInstructionAddress = UCInstructionAddress::ZERO;
    pub const MSRAM_START: UCInstructionAddress = UCInstructionAddress::from_const(0x7c00);
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

impl From<&UCInstructionAddress> for UCInstructionAddress {
    fn from(value: &UCInstructionAddress) -> Self {
        *value
    }
}

/// An address of a location in the patch RAM.
/// This address is used when writing or reading patch code.
/// Memory layout is like this:
/// [0, 1, ..., 80, 81, ..., 100, 101, ...]
/// which maps to the following ucode addresses
/// [7C00, 7C04, ..., 7C01, 7C05, ..., 7C02, 7C06, ...]
/// todo: question does jumps exist from addresses ending by 0b01?
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MSRAMInstructionPartWriteAddress(usize);
impl Address for MSRAMInstructionPartWriteAddress {
    fn address(&self) -> usize {
        self.0
    }
}
impl MSRAMInstructionPartWriteAddress {
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
    pub const ZERO: MSRAMInstructionPartWriteAddress =
        MSRAMInstructionPartWriteAddress::from_const(0);
    pub const MIN: MSRAMInstructionPartWriteAddress = MSRAMInstructionPartWriteAddress::ZERO;
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

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MSRAMInstructionPartReadAddress(usize);
impl Address for MSRAMInstructionPartReadAddress {
    fn address(&self) -> usize {
        self.0
    }
}
impl MSRAMInstructionPartReadAddress {
    #[track_caller]
    pub const fn from_const(value: usize) -> Self {
        if value >= 128 * 3 {
            panic!("Memory out out bounds. Address value is larger than limit 1FF.")
        }

        MSRAMInstructionPartReadAddress(value)
    }
    pub const ZERO: MSRAMInstructionPartReadAddress =
        MSRAMInstructionPartReadAddress::from_const(0);
    pub const MIN: MSRAMInstructionPartReadAddress = MSRAMInstructionPartReadAddress::ZERO;
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

/// An address of a location in the sequence word RAM.
/// This address is used when writing or reading SEQW patch code.
/// 3 ucode instructions share a single sequence word.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MSRAMSequenceWordAddress(usize);
impl Address for MSRAMSequenceWordAddress {
    fn address(&self) -> usize {
        self.0
    }
}
impl MSRAMSequenceWordAddress {
    #[track_caller]
    pub const fn from_const(value: usize) -> Self {
        if value >= 128 {
            panic!("Address out of bounds exception. Address is larger than limit 0x7F.");
        }

        MSRAMSequenceWordAddress(value)
    }
    pub const ZERO: MSRAMSequenceWordAddress = MSRAMSequenceWordAddress::from_const(0);
    pub const MIN: MSRAMSequenceWordAddress = MSRAMSequenceWordAddress::ZERO;
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

/// A patch index address. In the hook RAM hooks are labeled with an index.
/// This address is used when writing or reading patch hooks.
/// Patch indexes are multiples of 1 and start at 0. Till 31
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MSRAMHookIndex(usize);
impl Address for MSRAMHookIndex {
    fn address(&self) -> usize {
        self.0 * 2
    }
}
impl MSRAMHookIndex {
    #[track_caller]
    pub const fn from_const(value: usize) -> Self {
        if value >= 32 {
            panic!("Index out of bounds exception. Index must be smaller than 32.")
        }

        MSRAMHookIndex(value)
    }
    pub const ZERO: Self = MSRAMHookIndex::from_const(0);
    pub const MIN: Self = MSRAMHookIndex::ZERO;
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
