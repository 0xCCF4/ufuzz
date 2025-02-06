use crate::hardware_vt::GuestRegisters;
use alloc::boxed::Box;
use alloc::vec::Vec;
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

pub trait StateDifference {
    fn difference<'a, 'b>(
        &'a self,
        other: &'b Self,
    ) -> Vec<(&'static str, Box<&'a dyn Debug>, Box<&'b dyn Debug>)>;
}

impl StateDifference for VmState {
    fn difference<'a, 'b>(
        &'a self,
        other: &'b Self,
    ) -> Vec<(&'static str, Box<&'a dyn Debug>, Box<&'b dyn Debug>)> {
        let mut differences: Vec<(&'static str, Box<&'a dyn Debug>, Box<&'b dyn Debug>)> =
            Vec::new();

        // todo implement reflection etc

        // standard registers
        if self.standard_registers != other.standard_registers {
            let this = &self.standard_registers;
            let other = &other.standard_registers;

            if this.rax != other.rax {
                differences.push(("rax", Box::new(&this.rax), Box::new(&other.rax)));
            }
            if this.rbx != other.rbx {
                differences.push(("rbx", Box::new(&this.rbx), Box::new(&other.rbx)));
            }
            if this.rcx != other.rcx {
                differences.push(("rcx", Box::new(&this.rcx), Box::new(&other.rcx)));
            }
            if this.rdx != other.rdx {
                differences.push(("rdx", Box::new(&this.rdx), Box::new(&other.rdx)));
            }
            if this.rsi != other.rsi {
                differences.push(("rsi", Box::new(&this.rsi), Box::new(&other.rsi)));
            }
            if this.rdi != other.rdi {
                differences.push(("rdi", Box::new(&this.rdi), Box::new(&other.rdi)));
            }
            if this.rbp != other.rbp {
                differences.push(("rbp", Box::new(&this.rbp), Box::new(&other.rbp)));
            }
            if this.rsp != other.rsp {
                differences.push(("rsp", Box::new(&this.rsp), Box::new(&other.rsp)));
            }
            if this.r8 != other.r8 {
                differences.push(("r8", Box::new(&this.r8), Box::new(&other.r8)));
            }
            if this.r9 != other.r9 {
                differences.push(("r9", Box::new(&this.r9), Box::new(&other.r9)));
            }
            if this.r10 != other.r10 {
                differences.push(("r10", Box::new(&this.r10), Box::new(&other.r10)));
            }
            if this.r11 != other.r11 {
                differences.push(("r11", Box::new(&this.r11), Box::new(&other.r11)));
            }
            if this.r12 != other.r12 {
                differences.push(("r12", Box::new(&this.r12), Box::new(&other.r12)));
            }
            if this.r13 != other.r13 {
                differences.push(("r13", Box::new(&this.r13), Box::new(&other.r13)));
            }
            if this.r14 != other.r14 {
                differences.push(("r14", Box::new(&this.r14), Box::new(&other.r14)));
            }
            if this.r15 != other.r15 {
                differences.push(("r15", Box::new(&this.r15), Box::new(&other.r15)));
            }
            if this.rip != other.rip {
                differences.push(("rip", Box::new(&this.rip), Box::new(&other.rip)));
            }
            if this.rflags != other.rflags {
                differences.push(("rflags", Box::new(&this.rflags), Box::new(&other.rflags)));
            }
            if this.xmm0 != other.xmm0 {
                differences.push(("xmm0", Box::new(&this.xmm0), Box::new(&other.xmm0)));
            }
            if this.xmm1 != other.xmm1 {
                differences.push(("xmm1", Box::new(&this.xmm1), Box::new(&other.xmm1)));
            }
            if this.xmm2 != other.xmm2 {
                differences.push(("xmm2", Box::new(&this.xmm2), Box::new(&other.xmm2)));
            }
            if this.xmm3 != other.xmm3 {
                differences.push(("xmm3", Box::new(&this.xmm3), Box::new(&other.xmm3)));
            }
            if this.xmm4 != other.xmm4 {
                differences.push(("xmm4", Box::new(&this.xmm4), Box::new(&other.xmm4)));
            }
            if this.xmm5 != other.xmm5 {
                differences.push(("xmm5", Box::new(&this.xmm5), Box::new(&other.xmm5)));
            }
            if this.xmm6 != other.xmm6 {
                differences.push(("xmm6", Box::new(&this.xmm6), Box::new(&other.xmm6)));
            }
            if this.xmm7 != other.xmm7 {
                differences.push(("xmm7", Box::new(&this.xmm7), Box::new(&other.xmm7)));
            }
            if this.xmm8 != other.xmm8 {
                differences.push(("xmm8", Box::new(&this.xmm8), Box::new(&other.xmm8)));
            }
            if this.xmm9 != other.xmm9 {
                differences.push(("xmm9", Box::new(&this.xmm9), Box::new(&other.xmm9)));
            }
            if this.xmm10 != other.xmm10 {
                differences.push(("xmm10", Box::new(&this.xmm10), Box::new(&other.xmm10)));
            }
            if this.xmm11 != other.xmm11 {
                differences.push(("xmm11", Box::new(&this.xmm11), Box::new(&other.xmm11)));
            }
            if this.xmm12 != other.xmm12 {
                differences.push(("xmm12", Box::new(&this.xmm12), Box::new(&other.xmm12)));
            }
            if this.xmm13 != other.xmm13 {
                differences.push(("xmm13", Box::new(&this.xmm13), Box::new(&other.xmm13)));
            }
            if this.xmm14 != other.xmm14 {
                differences.push(("xmm14", Box::new(&this.xmm14), Box::new(&other.xmm14)));
            }
            if this.xmm15 != other.xmm15 {
                differences.push(("xmm15", Box::new(&this.xmm15), Box::new(&other.xmm15)));
            }
        }

        if self.extended_registers != other.extended_registers {
            let this = &self.extended_registers;
            let other = &other.extended_registers;

            if this.gdtr != other.gdtr {
                differences.push(("gdtr", Box::new(&this.gdtr), Box::new(&other.gdtr)));
            }
            if this.idtr != other.idtr {
                differences.push(("idtr", Box::new(&this.idtr), Box::new(&other.idtr)));
            }
            if this.ldtr_base != other.ldtr_base {
                differences.push((
                    "ldtr_base",
                    Box::new(&this.ldtr_base),
                    Box::new(&other.ldtr_base),
                ));
            }
            if this.ldtr != other.ldtr {
                differences.push(("ldtr", Box::new(&this.ldtr), Box::new(&other.ldtr)));
            }
            if this.es != other.es {
                differences.push(("es", Box::new(&this.es), Box::new(&other.es)));
            }
            if this.cs != other.cs {
                differences.push(("cs", Box::new(&this.cs), Box::new(&other.cs)));
            }
            if this.ss != other.ss {
                differences.push(("ss", Box::new(&this.ss), Box::new(&other.ss)));
            }
            if this.ds != other.ds {
                differences.push(("ds", Box::new(&this.ds), Box::new(&other.ds)));
            }
            if this.fs != other.fs {
                differences.push(("fs", Box::new(&this.fs), Box::new(&other.fs)));
            }
            if this.gs != other.gs {
                differences.push(("gs", Box::new(&this.gs), Box::new(&other.gs)));
            }
            if this.tr != other.tr {
                differences.push(("tr", Box::new(&this.tr), Box::new(&other.tr)));
            }
            if this.efer != other.efer {
                differences.push(("efer", Box::new(&this.efer), Box::new(&other.efer)));
            }
            if this.cr0 != other.cr0 {
                differences.push(("cr0", Box::new(&this.cr0), Box::new(&other.cr0)));
            }
            if this.cr3 != other.cr3 {
                differences.push(("cr3", Box::new(&this.cr3), Box::new(&other.cr3)));
            }
            if this.cr4 != other.cr4 {
                differences.push(("cr4", Box::new(&this.cr4), Box::new(&other.cr4)));
            }
            if this.fs_base != other.fs_base {
                differences.push(("fs_base", Box::new(&this.fs_base), Box::new(&other.fs_base)));
            }
            if this.gs_base != other.gs_base {
                differences.push(("gs_base", Box::new(&this.gs_base), Box::new(&other.gs_base)));
            }
            if this.tr_base != other.tr_base {
                differences.push(("tr_base", Box::new(&this.tr_base), Box::new(&other.tr_base)));
            }
            if this.sysenter_cs != other.sysenter_cs {
                differences.push((
                    "sysenter_cs",
                    Box::new(&this.sysenter_cs),
                    Box::new(&other.sysenter_cs),
                ));
            }
            if this.sysenter_esp != other.sysenter_esp {
                differences.push((
                    "sysenter_esp",
                    Box::new(&this.sysenter_esp),
                    Box::new(&other.sysenter_esp),
                ));
            }
            if this.sysenter_eip != other.sysenter_eip {
                differences.push((
                    "sysenter_eip",
                    Box::new(&this.sysenter_eip),
                    Box::new(&other.sysenter_eip),
                ));
            }
            if this.dr7 != other.dr7 {
                differences.push(("dr7", Box::new(&this.dr7), Box::new(&other.dr7)));
            }
            if this.es_base != other.es_base {
                differences.push(("es_base", Box::new(&this.es_base), Box::new(&other.es_base)));
            }
            if this.cs_base != other.cs_base {
                differences.push(("cs_base", Box::new(&this.cs_base), Box::new(&other.cs_base)));
            }
            if this.ss_base != other.ss_base {
                differences.push(("ss_base", Box::new(&this.ss_base), Box::new(&other.ss_base)));
            }
            if this.ds_base != other.ds_base {
                differences.push(("ds_base", Box::new(&this.ds_base), Box::new(&other.ds_base)));
            }
        }

        differences
    }
}

impl VmState {
    pub fn null_addresses(mut self) -> VmState {
        self.standard_registers.rip = 0;
        self
    }

    pub fn is_equal_no_address_compare(&self, other: &Self) -> bool {
        self.standard_registers
            .is_equal_no_address_compare(&other.standard_registers)
            && self.extended_registers == other.extended_registers
    }
}
