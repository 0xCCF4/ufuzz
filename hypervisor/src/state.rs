use x86::dtables::DescriptorTablePointer;
use crate::hardware_vt::GuestRegisters;

#[derive(Debug, Default, Clone)]
pub struct VmState {
    pub standard_registers: GuestRegisters,
    pub extended_registers: VmStateExtendedRegisters,
}

/// 26.4.1 Guest Register State
#[derive(Debug, Default)]
pub struct VmStateExtendedRegisters {
    pub gdtr: x86::dtables::DescriptorTablePointer<u64>,
    pub idtr: x86::dtables::DescriptorTablePointer<u64>,
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

impl Clone for VmStateExtendedRegisters {
    fn clone(&self) -> Self {
        VmStateExtendedRegisters {
            gdtr: DescriptorTablePointer {
                base: self.gdtr.base,
                limit: self.gdtr.limit,
            },
            idtr: DescriptorTablePointer {
                base: self.idtr.base,
                limit: self.idtr.limit,
            },
            ldtr_base: self.ldtr_base,
            ldtr: self.ldtr,
            es: self.es,
            cs: self.cs,
            ss: self.ss,
            ds: self.ds,
            fs: self.fs,
            gs: self.gs,
            tr: self.tr,
            efer: self.efer,
            cr0: self.cr0,
            cr3: self.cr3,

            cr4: self.cr4,
            fs_base: self.fs_base,
            gs_base: self.gs_base,
            tr_base: self.tr_base,
            sysenter_cs: self.sysenter_cs,
            sysenter_esp: self.sysenter_esp,
            sysenter_eip: self.sysenter_eip,
            dr7: self.dr7,
            es_base: self.es_base,
            cs_base: self.cs_base,
            ss_base: self.ss_base,
            ds_base: self.ds_base,
        }
    }
}
