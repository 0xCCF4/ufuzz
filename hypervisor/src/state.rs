use crate::hardware_vt::GuestRegisters;
use core::fmt::Debug;
use core::ops::{Deref, DerefMut};
use x86::dtables::DescriptorTablePointer;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct VmState {
    pub standard_registers: GuestRegisters,
    pub extended_registers: VmStateExtendedRegisters,
}

/// 26.4.1 Guest Register State
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VmStateExtendedRegisters {
    pub gdtr: DescriptorTablePointerWrapper<u64>,
    pub idtr: DescriptorTablePointerWrapper<u64>,
    pub ldtr_base: u64,
    pub ldtr: u16,
    pub es: u16,
    pub cs: u16,
    pub ss: u16,
    pub ds: u16,
    pub fs: u16,
    pub gs: u16,
    pub tr: u16,
    pub efer: u64,
    pub cr0: u64,
    pub cr3: u64,
    pub cr4: u64,
    pub fs_base: u64,
    pub gs_base: u64,
    pub tr_base: u64,
    pub sysenter_cs: u64,
    pub sysenter_esp: u64,
    pub sysenter_eip: u64,
    pub dr7: u64,
    pub es_base: u64,
    pub cs_base: u64,
    pub ss_base: u64,
    pub ds_base: u64,
}

#[derive(Default)]
#[repr(transparent)]
pub struct DescriptorTablePointerWrapper<T>(pub DescriptorTablePointer<T>);

impl<T> From<DescriptorTablePointer<T>> for DescriptorTablePointerWrapper<T> {
    fn from(dtp: DescriptorTablePointer<T>) -> Self {
        DescriptorTablePointerWrapper(dtp)
    }
}

impl<T> Deref for DescriptorTablePointerWrapper<T> {
    type Target = DescriptorTablePointer<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for DescriptorTablePointerWrapper<u64> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> Debug for DescriptorTablePointerWrapper<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

impl<T> PartialEq for DescriptorTablePointerWrapper<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0.limit == other.0.limit && self.0.base as usize == other.0.base as usize
    }
}

impl<T> Clone for DescriptorTablePointerWrapper<T> {
    fn clone(&self) -> Self {
        DescriptorTablePointerWrapper(DescriptorTablePointer {
            base: self.0.base,
            limit: self.0.limit,
        })
    }
}

impl<T> Eq for DescriptorTablePointerWrapper<T> {}
