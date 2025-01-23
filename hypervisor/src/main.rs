#![feature(new_zeroed_alloc)]
#![no_main]
#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;
use hypervisor::hardware_vt::{GuestRegisters, NestedPagingStructureEntryType};
use hypervisor::state::{VmState, VmStateExtendedRegisters};
use hypervisor::vm::Vm;
use hypervisor::Page;
use iced_x86::code_asm::*;
use iced_x86::code_asm::{rax, CodeAssembler};
use iced_x86::OpKind::Register;
use log::{error, info};
use uefi::proto::pi::mp::MpServices;
use uefi::runtime::ResetType;
use uefi::{boot, entry, println, Status};
use x86::controlregs::{Cr0, Cr4};
use x86::current::paging::BASE_PAGE_SHIFT;
use x86::current::rflags::RFlags;
use x86::dtables::{sgdt, DescriptorTablePointer};
use x86::segmentation::{
    BuildDescriptor, CodeSegmentType, DataSegmentType, Descriptor, DescriptorBuilder,
    GateDescriptorBuilder, SegmentDescriptorBuilder, SegmentSelector,
};
use x86::Ring;

#[entry]
unsafe fn main() -> Status {
    uefi::helpers::init().unwrap();
    info!("Hello world!");

    let handle = match boot::get_handle_for_protocol::<MpServices>() {
        Ok(h) => h,
        Err(e) => {
            error!("Failed to get handle for protocol: {:?}", e);
            return Status::ABORTED;
        }
    };
    let mp_services = match boot::open_protocol_exclusive::<MpServices>(handle) {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to open protocol exclusive: {:?}", e);
            return Status::ABORTED;
        }
    };
    let processor_count = match mp_services.get_number_of_processors() {
        Ok(pc) => pc,
        Err(e) => {
            error!("Failed to get number of processors: {:?}", e);
            return Status::ABORTED;
        }
    };

    info!("Total processors: {}", processor_count.total);
    info!("Enabled processors: {}", processor_count.enabled);

    drop(mp_services);

    // MEMORY LAYOUT IN PAGES
    //
    // 0: code
    // 1: stack
    // 2: global descriptor table
    // 3: TSS
    const CODE_PAGE_INDEX: usize = 0;
    const STACK_PAGE_INDEX: usize = 1;
    const GDT_PAGE_INDEX: usize = 2;
    const TSS_PAGE_INDEX: usize = 3;

    let mut guest_memory = Box::<[Page]>::new_zeroed_slice(10).assume_init();

    let mut descriptors = Vec::new();
    descriptors.push(Descriptor::NULL);

    let code = DescriptorBuilder::code_descriptor(0, 0xfffff, CodeSegmentType::ExecuteReadAccessed)
        .present()
        .dpl(Ring::Ring0)
        .l()
        .finish();

    let code_index = descriptors.len();
    descriptors.push(code);

    let data = DescriptorBuilder::data_descriptor(0, 0xfffff, DataSegmentType::ReadWriteAccessed)
        .present()
        .dpl(Ring::Ring0)
        .l()
        .finish();
    let data_index = descriptors.len();
    let stack_index = data_index;
    descriptors.push(data);

    let tss_descriptor: Descriptor =
        <DescriptorBuilder as GateDescriptorBuilder<u32>>::tss_descriptor(
            2 << BASE_PAGE_SHIFT,
            100,
            false,
        )
        .present()
        .dpl(Ring::Ring0)
        .finish();

    let tss_index = descriptors.len();
    descriptors.push(tss_descriptor);
    descriptors.push(Descriptor::NULL); // 64bit mode

    let descriptor_page = guest_memory[GDT_PAGE_INDEX].as_slice_mut();
    for (i, descriptor) in descriptors.into_iter().enumerate() {
        let descriptor_ptr = descriptor_page.as_mut_ptr().cast::<u64>().add(i);
        core::ptr::write(descriptor_ptr, descriptor.as_u64());
    }

    let mut standard_registers = GuestRegisters::default();
    standard_registers.rip = (CODE_PAGE_INDEX as u64) << BASE_PAGE_SHIFT;
    standard_registers.rsp = ((STACK_PAGE_INDEX as u64 + 1) << BASE_PAGE_SHIFT) - 8;
    standard_registers.rflags =
        (RFlags::FLAGS_A1 | RFlags::FLAGS_ID | RFlags::FLAGS_IF | RFlags::FLAGS_IOPL0).bits()
            as u64;

    let extended_registers = VmStateExtendedRegisters {
        gdtr: DescriptorTablePointer {
            base: descriptor_page.as_ptr().cast(),
            limit: 2,
        },
        idtr: DescriptorTablePointer::default(),
        ldtr_base: 0,
        ldtr: 0,
        cs: SegmentSelector::new(code_index as u16, Ring::Ring0).bits(),
        ss: SegmentSelector::new(stack_index as u16, Ring::Ring0).bits(),
        ds: SegmentSelector::new(data_index as u16, Ring::Ring0).bits(),
        es: SegmentSelector::new(0, Ring::Ring0).bits(),
        fs: SegmentSelector::new(0, Ring::Ring0).bits(),
        gs: SegmentSelector::new(0, Ring::Ring0).bits(),
        tr: SegmentSelector::new(tss_index as u16, Ring::Ring0).bits(),
        efer: 1 << 8, // 64bit/32e, no syscall, no execute disable
        cr0: (Cr0::CR0_PROTECTED_MODE).bits() as u64, // cache enabled, write protect disabled, 32(e)bit
        cr3: 0,
        cr4: Cr4::CR4_ENABLE_PAE.bits() as u64, // enable 32e
        fs_base: 0,
        gs_base: 0,
        tr_base: 0,
        es_base: 0,
        cs_base: 0,
        ss_base: 0,
        ds_base: 0,
        sysenter_cs: 0,
        sysenter_esp: 0,
        sysenter_eip: 0,
        dr7: 0,
    };

    let mut state = VmState {
        standard_registers,
        extended_registers,
    };

    let code_page = guest_memory[CODE_PAGE_INDEX].as_slice_mut();

    //code_page[0] = 0xF4; // hlt

    //code_page[0] = 0x0F;
    //code_page[1] = 0x0B; // ud2

    //code_page[0] = 0xCC; // int3

    //code_page[0] = 0xCD; // int5
    //code_page[1] = 0x05;

    let mut assembler = CodeAssembler::new(64).unwrap();

    assembler.push(rax).unwrap();
    assembler.mov(rax, dword_ptr(0x00)).unwrap();
    assembler.hlt().unwrap();

    let bytes = assembler
        .assemble((CODE_PAGE_INDEX << BASE_PAGE_SHIFT) as u64)
        .unwrap();
    for (src, dst) in bytes
        .iter()
        .zip(guest_memory[CODE_PAGE_INDEX].as_slice_mut().iter_mut())
    {
        *dst = *src;
    }

    let mut gdt: DescriptorTablePointer<Descriptor> = DescriptorTablePointer::default();
    sgdt(&mut gdt);

    let mut vm = Vm::new();
    vm.vt.enable();
    vm.initialize();

    let translations = [
        vm.build_translation(
            CODE_PAGE_INDEX << BASE_PAGE_SHIFT,
            guest_memory[CODE_PAGE_INDEX].as_ptr(),
            NestedPagingStructureEntryType::Rx,
        ),
        vm.build_translation(
            STACK_PAGE_INDEX << BASE_PAGE_SHIFT,
            guest_memory[STACK_PAGE_INDEX].as_ptr(),
            NestedPagingStructureEntryType::Rw,
        ),
        vm.build_translation(
            GDT_PAGE_INDEX << BASE_PAGE_SHIFT,
            guest_memory[GDT_PAGE_INDEX].as_ptr(),
            NestedPagingStructureEntryType::R,
        ),
        vm.build_translation(
            TSS_PAGE_INDEX << BASE_PAGE_SHIFT,
            guest_memory[TSS_PAGE_INDEX].as_ptr(),
            NestedPagingStructureEntryType::R,
        ),
    ];

    for t in translations.into_iter() {
        if let Err(err) = t {
            error!("Failed to build translation: {:?}", err);
            return Status::ABORTED;
        }
    }

    vm.vt.load_state(&state);

    println!("Running vm");
    vm.vt.set_preemption_timer(1e8 as u64);

    let exit_reason = vm.vt.run();

    println!("Exit reason: {:#x?}", exit_reason);
    vm.vt.save_state(&mut state);
    println!("Rax: {:x?}", state.standard_registers.rax);

    println!("Goodbye!");
    uefi::runtime::reset(ResetType::SHUTDOWN, Status::SUCCESS, None);
    // Status::SUCCESS
}
