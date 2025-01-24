use crate::hardware_vt::GuestRegisters;

#[derive(Debug, Default)]
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
