#![feature(new_zeroed_alloc)]
#![no_main]
#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use hypervisor::hardware_vt::NestedPagingStructureEntryType;
use hypervisor::state::{GuestRegisters, VmState, VmStateExtendedRegisters};
use hypervisor::vm::Vm;
use hypervisor::x86_instructions::sgdt;
use hypervisor::Page;
use iced_x86::code_asm::CodeAssembler;
use iced_x86::{
    code_asm, Decoder, DecoderOptions, Formatter, IcedError, Instruction, NasmFormatter,
};
use log::{error, info};
use uefi::proto::pi::mp::MpServices;
use uefi::{boot, entry, print, println, Status};
use x86::bits64::paging::{PAddr, PDPTEntry, PDPTFlags, PML4Entry, PML4Flags, BASE_PAGE_SHIFT};
use x86::controlregs::{Cr0, Cr4};
use x86::current::rflags::RFlags;
use x86::dtables::DescriptorTablePointer;
use x86::segmentation::{
    BuildDescriptor, CodeSegmentType, DataSegmentType, Descriptor, DescriptorBuilder,
    GateDescriptorBuilder, SegmentDescriptorBuilder, SegmentSelector,
};
use x86::Ring;

const _: () = assert!(size_of::<PML4Entry>() == 8);
const _: () = assert!(size_of::<PDPTEntry>() == 8);
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
    // 4-5: page tables
    const CODE_PAGE_INDEX: usize = 0;
    const STACK_PAGE_INDEX: usize = 1;
    const GDT_PAGE_INDEX: usize = 2;
    const TSS_PAGE_INDEX: usize = 3;
    const PAGE_TABLE_4_INDEX: usize = 4;
    const PAGE_TABLE_3_INDEX: usize = 5;

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
        }
        .into(),
        idtr: DescriptorTablePointer::default().into(),
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
        cr0: (Cr0::CR0_PROTECTED_MODE | Cr0::CR0_ENABLE_PAGING/* TODO REMOVE */).bits() as u64, // cache enabled, write protect disabled, 32(e)bit
        cr3: (PAGE_TABLE_4_INDEX << BASE_PAGE_SHIFT) as u64,
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

    // build page table
    let mut page_table_4: *mut PML4Entry = guest_memory[PAGE_TABLE_4_INDEX].as_mut_ptr().cast();
    for i in 0..512 {
        let mut dst_entry = page_table_4.add(i);

        let entry = PML4Entry::new(
            PAddr((PAGE_TABLE_3_INDEX << BASE_PAGE_SHIFT) as u64),
            PML4Flags::P | PML4Flags::RW | PML4Flags::US,
        );
        *dst_entry = entry;
    }

    let mut page_table_3: *mut PDPTEntry = guest_memory[PAGE_TABLE_3_INDEX].as_mut_ptr().cast();
    for i in 0..512 {
        let mut dst_entry = page_table_3.add(i);

        // 1GB page size
        let entry = PDPTEntry::new(
            PAddr((i << 30) as u64),
            PDPTFlags::P | PDPTFlags::RW | PDPTFlags::US | PDPTFlags::PS,
        );
        *dst_entry = entry;
    }

    let mut gdt: DescriptorTablePointer<Descriptor> = DescriptorTablePointer::default();
    sgdt(&mut gdt);

    info!("Rflags: {:#018x}", x86::current::rflags::read());

    let mut vm = Vm::new();
    vm.vt.enable();
    if let Err(err) = vm.initialize() {
        error!("Failed to initialize vm: {:?}", err);
        vm.vt.disable();
        return Status::ABORTED;
    }

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
        vm.build_translation(
            PAGE_TABLE_4_INDEX << BASE_PAGE_SHIFT,
            guest_memory[PAGE_TABLE_4_INDEX].as_ptr(),
            NestedPagingStructureEntryType::Rw, // todo: maybe bochs bug, works with R -> paging algo sets accessed bit in page table, #PF no raised in bochs -> investigate
        ),
        vm.build_translation(
            PAGE_TABLE_3_INDEX << BASE_PAGE_SHIFT,
            guest_memory[PAGE_TABLE_3_INDEX].as_ptr(),
            NestedPagingStructureEntryType::Rw,
        ),
    ];

    for t in translations.into_iter() {
        if let Err(err) = t {
            error!("Failed to build translation: {:?}", err);
            return Status::ABORTED;
        }
    }

    let state = state;

    run_vm_report_status(
        &mut vm,
        &state,
        &mut guest_memory,
        CODE_PAGE_INDEX,
        STACK_PAGE_INDEX,
        |a| {
            a.mov(code_asm::rax, 0x0123456789u64)?;
            a.hlt()?;

            Ok(())
        },
    );

    run_vm_report_status(
        &mut vm,
        &state,
        &mut guest_memory,
        CODE_PAGE_INDEX,
        STACK_PAGE_INDEX,
        |a| {
            a.int3()?;

            Ok(())
        },
    );

    run_vm_report_status(
        &mut vm,
        &state,
        &mut guest_memory,
        CODE_PAGE_INDEX,
        STACK_PAGE_INDEX,
        |a| {
            a.jmp(0x0)?;

            Ok(())
        },
    );

    println!("Goodbye!");
    vm.vt.disable();
    println!("Final exit");
    info!("Rflags: {:#018x}", x86::current::rflags::read());
    //uefi::runtime::reset(ResetType::SHUTDOWN, Status::SUCCESS, None);
    Status::SUCCESS
}

fn run_vm_report_status<F: FnOnce(&mut CodeAssembler) -> Result<(), IcedError>>(
    vm: &mut Vm,
    state: &VmState,
    memory: &mut [Page],
    code_page: usize,
    stack_page: usize,
    code_gen: F,
) {
    vm.vt.load_state(&state);

    let code = compile_code(code_gen);
    let mut result_state = VmState::default();

    memory[stack_page]
        .as_slice_mut()
        .iter_mut()
        .for_each(|b| *b = 0);
    memory[code_page]
        .as_slice_mut()
        .iter_mut()
        .for_each(|b| *b = 0);
    for (src, dst) in code.iter().zip(memory[code_page].as_slice_mut().iter_mut()) {
        *dst = *src;
    }

    vm.vt.set_preemption_timer(1e8 as u64);

    let exit_reason = vm.vt.run();
    vm.vt.save_state(&mut result_state);

    println!("--------------------------");
    println!("Test scenario:");
    disassemble_code(&code);
    println!("Rax: {:x?}", result_state.standard_registers.rax);
    println!("Rip: {:x?}", result_state.standard_registers.rip);
    println!("Exit reason: {:#x?}", exit_reason);
}

fn compile_code<F: FnOnce(&mut CodeAssembler) -> Result<(), IcedError>>(code_gen: F) -> Vec<u8> {
    let mut assembler = CodeAssembler::new(64).unwrap();
    if let Err(err) = code_gen(&mut assembler) {
        error!("Code generation error: {:?}", err);
        return vec![0xf4 /* hlt */];
    }
    assembler.assemble(0).unwrap_or_else(|err| {
        error!("Assemble error: {:?}", err);
        vec![0xf4 /* hlt */]
    })
}

fn disassemble_code(code: &[u8]) {
    let mut decoder = Decoder::with_ip(64, code, 0, DecoderOptions::NONE);
    let mut formatter = NasmFormatter::new();

    formatter.options_mut().set_digit_separator("`");
    formatter.options_mut().set_first_operand_char_index(10);

    let mut output = String::new();
    let mut instruction = Instruction::default();

    while decoder.can_decode() {
        decoder.decode_out(&mut instruction);
        output.clear();
        formatter.format(&instruction, &mut output);

        // Eg. "00007FFAC46ACDB2 488DAC2400FFFFFF     lea       rbp,[rsp-100h]"
        print!("{:016X} ", instruction.ip());
        let start_index = (instruction.ip() - 0) as usize;
        let instr_bytes = &code[start_index..start_index + instruction.len()];
        for b in instr_bytes.iter() {
            print!("{:02X}", b);
        }
        if instr_bytes.len() < 10 {
            for _ in 0..10 - instr_bytes.len() {
                print!("  ");
            }
        }
        println!(" {}", output);
    }
}
