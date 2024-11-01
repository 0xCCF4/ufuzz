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

// A linear address. Starting at 0 counting up by 1 for each cell.
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
        LinearAddress(value)
    }
}
impl From<LinearAddress> for usize {
    fn from(value: LinearAddress) -> Self {
        value.0
    }
}
impl fmt::Display for LinearAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04x}", self.0)
    }
}
impl fmt::Debug for LinearAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04x}", self.0)
    }
}
impl Add<usize> for LinearAddress {
    type Output = Self;

    fn add(self, other: usize) -> Self {
        LinearAddress(self.0 + other)
    }
}
impl Sub<usize> for LinearAddress {
    type Output = Self;

    fn sub(self, other: usize) -> Self {
        LinearAddress(self.0 - other)
    }
}
impl Mul<usize> for LinearAddress {
    type Output = Self;

    fn mul(self, other: usize) -> Self {
        LinearAddress(self.0 * other)
    }
}
impl Div<usize> for LinearAddress {
    type Output = Self;

    fn div(self, other: usize) -> Self {
        LinearAddress(self.0 / other)
    }
}

// An address of a code instruction.
// Ucode instruction RAM starts at 0x7c00
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct UCInstructionAddress(usize);
impl Address for UCInstructionAddress {
    fn address(&self) -> usize {
        self.0
    }
}
impl UCInstructionAddress {
    pub const fn from_const(value: usize) -> Self {
        UCInstructionAddress(value)
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
        UCInstructionAddress(value.0)
    }
}
impl From<usize> for UCInstructionAddress {
    fn from(value: usize) -> Self {
        UCInstructionAddress(value)
    }
}
impl From<UCInstructionAddress> for usize {
    fn from(value: UCInstructionAddress) -> Self {
        value.0
    }
}
impl From<UCInstructionAddress> for LinearAddress {
    fn from(value: UCInstructionAddress) -> Self {
        LinearAddress(value.0)
    }
}
impl Add<usize> for UCInstructionAddress {
    type Output = Self;

    fn add(self, other: usize) -> Self {
        UCInstructionAddress(self.0 + other)
    }
}
impl Sub<usize> for UCInstructionAddress {
    type Output = Self;

    fn sub(self, other: usize) -> Self {
        UCInstructionAddress(self.0 - other)
    }
}

// An address of a location in the patch RAM.
// This address is used when writing or reading patch code.
// Addresses are multiples of 4 and start at 0.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MSRAMInstructionAddress(usize);
impl Address for MSRAMInstructionAddress {
    fn address(&self) -> usize {
        self.0
    }
}
impl MSRAMInstructionAddress {
    pub const fn from_const(value: usize) -> Self {
        MSRAMInstructionAddress(value & !0x3)
    }
}
impl MSRAMAddress for MSRAMInstructionAddress {}
impl fmt::Display for MSRAMInstructionAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "P{:04x}", self.0)
    }
}
impl fmt::Debug for MSRAMInstructionAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "P{:04x}", self.0)
    }
}
impl From<LinearAddress> for MSRAMInstructionAddress {
    fn from(value: LinearAddress) -> Self {
        let base = value.address();
        let offset = base % 4;
        let row = base / 4;
        // the last *4 does not make any sense but the CPU divides the address where
        // to write by 4, still unknown reasons
        MSRAMInstructionAddress((offset * 0x80 + row) * 4)
    }
}
impl From<UCInstructionAddress> for MSRAMInstructionAddress {
    fn from(value: UCInstructionAddress) -> Self {
        let addr = value.address().max(0x7c00);
        if addr < 0x7c00 {
            panic!("Address is not in the ucode RAM: {}", addr);
        }
        MSRAMInstructionAddress::from(LinearAddress::from(addr - 0x7c00))
    }
}
impl From<MSRAMInstructionAddress> for LinearAddress {
    fn from(value: MSRAMInstructionAddress) -> Self {
        let addr = value.0 / 4;
        let offset = addr / 0x80;
        let base = addr % 0x80;
        LinearAddress(base * 4 + offset)
    }
}
impl From<MSRAMInstructionAddress> for UCInstructionAddress {
    fn from(value: MSRAMInstructionAddress) -> Self {
        UCInstructionAddress::from(LinearAddress::from(value) + 0x7c00)
    }
}

// An address of a location in the sequence word RAM.
// This address is used when writing or reading SEQW patch code.
// 3 ucode instructions share a single sequence word.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MSRAMSequenceWordAddress(usize);
impl Address for MSRAMSequenceWordAddress {
    fn address(&self) -> usize {
        self.0
    }
}
impl MSRAMSequenceWordAddress {
    pub fn from_const(value: usize) -> Self {
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
        let addr: MSRAMInstructionAddress = value.into();
        MSRAMSequenceWordAddress((addr.0 / 4) % 0x80)
    }
}
impl From<LinearAddress> for MSRAMSequenceWordAddress {
    fn from(value: LinearAddress) -> Self {
        //MSRAMSequenceWordAddress(MSRAMInstructionAddress::from(value * 4))
        todo!("Implement")
    }
}
impl From<MSRAMSequenceWordAddress> for UCInstructionAddress {
    fn from(value: MSRAMSequenceWordAddress) -> Self {
        UCInstructionAddress::from(MSRAMInstructionAddress(value.0 * 4))
    }
}
impl From<MSRAMSequenceWordAddress> for LinearAddress {
    fn from(value: MSRAMSequenceWordAddress) -> Self {
        LinearAddress::from(MSRAMInstructionAddress(value.0 * 4)) // TODO check, likely wrong
    }
}

// A patch index address. In the hook RAM hooks are labeled with an index.
// This address is used when writing or reading patch hooks.
// Patch indexes are multiples of 2 and start at 0.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MSRAMHookAddress(usize);
impl Address for MSRAMHookAddress {
    fn address(&self) -> usize {
        self.0
    }
}
impl MSRAMHookAddress {
    pub const fn from_const(value: usize) -> Self {
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
        MSRAMHookAddress(value.address() * 2)
    }
}
impl From<MSRAMHookAddress> for LinearAddress {
    fn from(value: MSRAMHookAddress) -> Self {
        LinearAddress(value.0 / 2)
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

    fn conversion_harness<A: Address + From<B>, B: Address + From<A>>(tests: &[(A, B)]) {
        for (a, b) in tests {
            assert_eq!(B::from(*a).address(), a.address());
            assert_eq!(A::from(*b).address(), b.address());
        }
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
}
