use crate::std::fmt;
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
    pub const fn from_const(value: usize) -> Self {
        LinearAddress(value)
    }
}
impl From<usize> for LinearAddress {
    fn from(value: usize) -> Self {
        LinearAddress::from_const(value)
    }
}
impl From<LinearAddress> for usize {
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

    fn add(self, other: usize) -> Self {
        LinearAddress::from_const(self.0 + other)
    }
}
impl Sub<usize> for LinearAddress {
    type Output = Self;

    fn sub(self, other: usize) -> Self {
        LinearAddress::from(self.0 - other)
    }
}
impl Mul<usize> for LinearAddress {
    type Output = Self;

    fn mul(self, other: usize) -> Self {
        LinearAddress::from_const(self.0 * other)
    }
}
impl Div<usize> for LinearAddress {
    type Output = Self;

    fn div(self, other: usize) -> Self {
        LinearAddress::from_const(self.0 / other)
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
    pub const fn from_const(value: usize) -> Self {
        if value > 0x7c00 + 3*128 {
            //panic!("Address out of bounds exception?? TODO recheck")
        }

        UCInstructionAddress(value)
    }

    pub fn patch_address(&self, offset: usize) -> MSRAMInstructionPartAddress {
        MSRAMInstructionPartAddress::from(LinearAddress::from(MSRAMInstructionPartAddress::from(*self)).add(offset))
    }
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
    fn from(value: LinearAddress) -> Self {
        UCInstructionAddress::from_const(value.0)
    }
}
impl From<usize> for UCInstructionAddress {
    fn from(value: usize) -> Self {
        UCInstructionAddress::from_const(value)
    }
}
impl From<UCInstructionAddress> for usize {
    fn from(value: UCInstructionAddress) -> Self {
        value.0
    }
}
impl From<UCInstructionAddress> for LinearAddress {
    fn from(value: UCInstructionAddress) -> Self {
        LinearAddress::from_const(value.0)
    }
}
impl Add<usize> for UCInstructionAddress {
    type Output = Self;

    fn add(self, other: usize) -> Self {
        UCInstructionAddress::from_const(self.0 + other)
    }
}
impl Sub<usize> for UCInstructionAddress {
    type Output = Self;

    fn sub(self, other: usize) -> Self {
        UCInstructionAddress::from_const(self.0 - other)
    }
}

/// An address of a location in the patch RAM.
/// This address is used when writing or reading patch code.
/// Addresses start at 0, incrementing by one, skipping each forth value
/// 0 corresponds to U7c00
/// A mapping from Linear to InstructionAddress looks like this:
/// [0,1,2,3,4,5,...] -> [0,1,2,4,5,6,8,...]
/// Since each forth entry is not available
/// Instructions map like this
/// 7c00+[0,1,2,4,5,6,8,9,a,...] -> [0,1,2,4,5,6,8,9,a,...]
/// todo: question does jumps exist from addresses ending by 0b01?
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MSRAMInstructionPartAddress(usize);
impl Address for MSRAMInstructionPartAddress {
    fn address(&self) -> usize {
        self.0
    }
}
impl MSRAMInstructionPartAddress {
    pub const fn from_const(value: usize) -> Self {
        if (value & 3) == 3 {
            // panic!("Address invalid")
        }
        MSRAMInstructionPartAddress(value)
    }
}
impl MSRAMAddress for MSRAMInstructionPartAddress {}
impl fmt::Display for MSRAMInstructionPartAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "I{:04x}", self.0)
    }
}
impl fmt::Debug for MSRAMInstructionPartAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "I{:04x}", self.0)
    }
}
impl From<LinearAddress> for MSRAMInstructionPartAddress {
    fn from(value: LinearAddress) -> Self {
        // see custom processing unit
        let base = value.address();

        let mult = base / 3;
        let offset = base % 3;

        MSRAMInstructionPartAddress::from_const(mult * 4 + offset)
    }
}
impl From<UCInstructionAddress> for MSRAMInstructionPartAddress {
    fn from(value: UCInstructionAddress) -> Self {
        let addr = value.address();
        if addr < 0x7c00 {
            panic!("Address is not in the ucode RAM: {}", addr);
        }
        MSRAMInstructionPartAddress::from_const(addr - 0x7c00)
    }
}
impl From<MSRAMInstructionPartAddress> for LinearAddress {
    fn from(value: MSRAMInstructionPartAddress) -> Self {
        let addr = value.0;

        let mult = addr / 4;
        let offset = addr % 4;

        LinearAddress::from_const(mult * 3 + offset)
    }
}
impl From<MSRAMInstructionPartAddress> for UCInstructionAddress {
    fn from(value: MSRAMInstructionPartAddress) -> Self {
        UCInstructionAddress::from(LinearAddress::from(value) + 0x7c00)
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
    pub fn from_const(value: usize) -> Self {
        if value > 128 {
            panic!("Address out of bounds exception");
        }

        MSRAMSequenceWordAddress(value)
    }
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
    fn from(value: UCInstructionAddress) -> Self {
        let addr: MSRAMInstructionPartAddress = value.into();
        MSRAMSequenceWordAddress::from_const(addr.0 / 4)
    }
}
impl From<LinearAddress> for MSRAMSequenceWordAddress {
    fn from(value: LinearAddress) -> Self {
        let addr = value.address();
        MSRAMSequenceWordAddress::from_const(addr)
    }
}
impl From<MSRAMSequenceWordAddress> for UCInstructionAddress {
    fn from(value: MSRAMSequenceWordAddress) -> Self {
        UCInstructionAddress::from(MSRAMInstructionPartAddress(value.0 * 4))
    }
}
impl From<MSRAMSequenceWordAddress> for LinearAddress {
    fn from(value: MSRAMSequenceWordAddress) -> Self {
        LinearAddress::from(value.0)
    }
}
impl Add<usize> for MSRAMSequenceWordAddress {
    type Output = MSRAMSequenceWordAddress;
    fn add(self, rhs: usize) -> Self::Output {
        MSRAMSequenceWordAddress::from_const(self.0 + rhs)
    }
}

/// A patch index address. In the hook RAM hooks are labeled with an index.
/// This address is used when writing or reading patch hooks.
/// Patch indexes are multiples of 2 and start at 0.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MSRAMHookAddress(usize);
impl Address for MSRAMHookAddress {
    fn address(&self) -> usize {
        self.0
    }
}
impl MSRAMHookAddress {
    pub const fn from_const(value: usize) -> Self {
        if value > 64*2 {
            panic!("Address out of bounds exception")
        }

        MSRAMHookAddress(value & !1usize)
    }
    pub const ZERO: Self = MSRAMHookAddress(0);
}
impl MSRAMAddress for MSRAMHookAddress {}
impl fmt::Display for MSRAMHookAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "H{:04x}", self.0)
    }
}
impl fmt::Debug for MSRAMHookAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "H{:04x}", self.0)
    }
}
impl From<LinearAddress> for MSRAMHookAddress {
    fn from(value: LinearAddress) -> Self {
        MSRAMHookAddress::from_const(value.address() * 2)
    }
}
impl From<MSRAMHookAddress> for LinearAddress {
    fn from(value: MSRAMHookAddress) -> Self {
        LinearAddress::from_const(value.0 / 2)
    }
}

impl Add<usize> for MSRAMHookAddress {
    type Output = Self;

    fn add(self, other: usize) -> Self {
        MSRAMHookAddress::from_const(self.0 + other*2)
    }
}

impl Sub<usize> for MSRAMHookAddress {
    type Output = Self;

    fn sub(self, other: usize) -> Self {
        MSRAMHookAddress::from_const(self.0 - other*2)
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
            assert_eq!(b_from_a.address(), b.address(), "Converting A to B: {:04x} -> expected {:04x} got {:04x}", a.address(), b.address(), b_from_a.address());
            assert_eq!(a_from_b.address(), a.address(), "Converting B to A: {:04x} -> expected {:04x} got {:04x}", b.address(), a.address(), a_from_b.address());
        }
    }

    fn ucode_addr_to_patch_addr(ucode_addr: usize) -> usize {
        // from custom processing unit
        let base = ucode_addr - 0x7c00;
        let offset = base % 4;
        let row = base / 4;
        (offset * 0x80 + row) * 4
    }

    fn ucode_addr_to_patch_seqword_addr(addr: usize) -> usize {
        let base = addr - 0x7c00;
        let seq_addr = (base%4) * 0x80 + (base/4);
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
        let mut tests = Vec::default();
        for i in 0..0xFF {
            let la = LinearAddress(i);
            let patch_addr = MSRAMInstructionPartAddress::from_const(ucode_addr_to_patch_addr(i+0x7c00));
            tests.push((la, patch_addr));
        }
        conversion_harness(&tests);
    }

    #[test]
    fn test_convert_ua_inst_patch() {
        let mut tests = Vec::default();
        for i in 0x7c00..0x7c00+0xFF {
            let la = UCInstructionAddress(i);
            let patch_addr = MSRAMInstructionPartAddress::from_const(ucode_addr_to_patch_addr(i));
            tests.push((la, patch_addr));
        }
        conversion_harness(&tests);
    }

    #[test]
    fn test_convert_la_seqw_patch() {
        let mut tests = Vec::default();

        for i in 0..0x1f {
            let la = LinearAddress(i);
            let patch_addr = MSRAMSequenceWordAddress::from_const(ucode_addr_to_patch_seqword_addr(4*i+0x7c00));
            tests.push((la, patch_addr));
        }
        conversion_harness(&tests);
    }

    #[test]
    fn test_convert_uc_seqw_patch() {
        let mut tests = Vec::default();

        for i in 0x7c00..0x7c00+0xFF {
            let la = UCInstructionAddress(i & !0x3);
            let patch_addr = MSRAMSequenceWordAddress::from_const(ucode_addr_to_patch_seqword_addr(i));
            tests.push((la, patch_addr));
        }
        conversion_harness(&tests);
    }

    #[test]
    fn test_convert_la_hook() {
        let tests = vec![
            (LinearAddress(0), MSRAMHookAddress(0)),
            (LinearAddress(1), MSRAMHookAddress(2)),
            (LinearAddress(2), MSRAMHookAddress(4)),
            (LinearAddress(3), MSRAMHookAddress(6)),
            (LinearAddress(4), MSRAMHookAddress(8)),
        ];
        conversion_harness(&tests);
    }
}
