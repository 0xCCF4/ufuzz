use crate::{StateTrace, Trace};
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::pin::Pin;
use coverage::interface_definition::ComInterfaceDescription;
use fuzzer_data::MemoryAccess;
use hypervisor::error::HypervisorError;
use hypervisor::hardware_vt::NestedPagingStructureEntryType;
use hypervisor::state::{ExceptionQualification, GuestException, GuestRegisters, VmExitReason};
use hypervisor::state::{VmState, VmStateExtendedRegisters};
use hypervisor::vm::Vm;
use hypervisor::x86_instructions::sgdt;
use hypervisor::Page;
use iced_x86::code_asm;
use iced_x86::code_asm::CodeAssembler;
use itertools::Itertools;
use log::error;
#[cfg(feature = "__debug_performance_trace")]
use performance_timing::track_time;
use x86::bits64::paging::{PAddr, PDPTEntry, PDPTFlags, PML4Entry, PML4Flags, BASE_PAGE_SHIFT};
use x86::bits64::rflags::RFlags;
use x86::controlregs::{Cr0, Cr4};
use x86::dtables::DescriptorTablePointer;
use x86::segmentation::{
    BuildDescriptor, CodeSegmentType, DataSegmentType, Descriptor, DescriptorBuilder,
    GateDescriptorBuilder, SegmentDescriptorBuilder, SegmentSelector,
};
use x86::Ring;

pub struct Hypervisor {
    memory_code_page: Pin<Box<Page>>,
    memory_stack_page: Pin<Box<Page>>,

    #[allow(dead_code)]
    memory_code_entry_page: Pin<Box<Page>>,

    #[allow(dead_code)]
    memory_gdt_page: Pin<Box<Page>>,
    #[allow(dead_code)]
    memory_tss_page: Pin<Box<Page>>,
    #[allow(dead_code)]
    memory_page_table_4: Pin<Box<Page>>,
    #[allow(dead_code)]
    memory_page_table_3: Pin<Box<Page>>,

    vm: Vm,
    pub initial_state: VmState,
}

// MEMORY LAYOUT IN PAGES
//
// 0: code
// 1: stack
// 2: global descriptor table
// 3: TSS
// 4-5: page tables
// 6: code entry page, execution starts here

const CODE_PAGE_INDEX: usize = 0;
const COVERAGE_PAGE_INDEX: usize = 1;
const STACK_PAGE_INDEX: usize = 2;
const GDT_PAGE_INDEX: usize = 3;
const TSS_PAGE_INDEX: usize = 4;
const PAGE_TABLE_4_INDEX: usize = 5;
const PAGE_TABLE_3_INDEX: usize = 6;
const CODE_ENTRY_PAGE_INDEX: usize = 7;

#[cfg_attr(feature = "__debug_performance_trace", track_time)]
impl Hypervisor {
    pub fn new(
        coverage_interface: &'static ComInterfaceDescription,
    ) -> Result<Self, HypervisorError> {
        let code_page = Page::alloc_zeroed();
        let mut code_entry_page = Page::alloc_zeroed();
        let stack_page = Page::alloc_zeroed();
        let mut gdt_page = Page::alloc_zeroed();
        let tss_page = Page::alloc_zeroed();
        let mut page_table_4_page = Page::alloc_zeroed();
        let mut page_table_3_page = Page::alloc_zeroed();

        let mut descriptors = Vec::new();
        descriptors.push(Descriptor::NULL);

        let code =
            DescriptorBuilder::code_descriptor(0, 0xfffff, CodeSegmentType::ExecuteReadAccessed)
                .present()
                .dpl(Ring::Ring0)
                .l()
                .finish();

        let code_index = descriptors.len();
        descriptors.push(code);

        let data =
            DescriptorBuilder::data_descriptor(0, 0xfffff, DataSegmentType::ReadWriteAccessed)
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

        let descriptor_page = gdt_page.as_slice_mut();
        unsafe {
            for (i, descriptor) in descriptors.into_iter().enumerate() {
                let descriptor_ptr = descriptor_page.as_mut_ptr().cast::<u64>().add(i);
                core::ptr::write(descriptor_ptr, descriptor.as_u64());
            }
        }

        let code_entry = Self::generate_code_entry(
            (CODE_PAGE_INDEX << BASE_PAGE_SHIFT) as u64,
            (CODE_ENTRY_PAGE_INDEX << BASE_PAGE_SHIFT) as u64,
        );
        unsafe {
            core::ptr::copy_nonoverlapping(
                code_entry.as_ptr(),
                code_entry_page.as_slice_mut().as_mut_ptr(),
                code_entry.len().min(4096),
            );
        }

        let mut standard_registers = GuestRegisters::default();
        standard_registers.rip = (CODE_ENTRY_PAGE_INDEX as u64) << BASE_PAGE_SHIFT;
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
            cr0: (Cr0::CR0_PROTECTED_MODE | Cr0::CR0_ENABLE_PAGING).bits() as u64, // cache enabled, write protect disabled, 32(e)bit
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

        let state = VmState {
            standard_registers,
            extended_registers,
        };

        // build page table
        let page_table_4: *mut PML4Entry = page_table_4_page.as_mut_ptr().cast();
        unsafe {
            for i in 0..512 {
                let dst_entry = page_table_4.add(i);

                let entry = PML4Entry::new(
                    PAddr((PAGE_TABLE_3_INDEX << BASE_PAGE_SHIFT) as u64),
                    PML4Flags::P | PML4Flags::RW | PML4Flags::US | PML4Flags::A,
                );
                *dst_entry = entry;
            }
        }

        let page_table_3: *mut PDPTEntry = page_table_3_page.as_mut_ptr().cast();
        unsafe {
            for i in 0..512 {
                let dst_entry = page_table_3.add(i);

                // 1GB page size
                let entry = PDPTEntry::new(
                    PAddr((i << 30) as u64),
                    PDPTFlags::P
                        | PDPTFlags::RW
                        | PDPTFlags::US
                        | PDPTFlags::PS
                        | PDPTFlags::A
                        | PDPTFlags::D,
                );
                *dst_entry = entry;
            }
        }

        let mut gdt: DescriptorTablePointer<Descriptor> = DescriptorTablePointer::default();
        sgdt(&mut gdt);

        let mut vm = Vm::new();
        vm.vt.enable();
        if let Err(err) = vm.initialize() {
            error!("Failed to initialize vm: {:?}", err);
            vm.vt.disable();
            return Err(HypervisorError::FailedToInitializeHost(err));
        }

        let translations = [
            vm.build_translation(
                CODE_PAGE_INDEX << BASE_PAGE_SHIFT,
                code_page.as_ptr(),
                NestedPagingStructureEntryType::X,
            ),
            vm.build_translation(
                COVERAGE_PAGE_INDEX << BASE_PAGE_SHIFT,
                coverage_interface.base as *const Page,
                NestedPagingStructureEntryType::Rw,
            ),
            vm.build_translation(
                STACK_PAGE_INDEX << BASE_PAGE_SHIFT,
                stack_page.as_ptr(),
                NestedPagingStructureEntryType::Rw,
            ),
            vm.build_translation(
                GDT_PAGE_INDEX << BASE_PAGE_SHIFT,
                gdt_page.as_ptr(),
                NestedPagingStructureEntryType::R,
            ),
            vm.build_translation(
                TSS_PAGE_INDEX << BASE_PAGE_SHIFT,
                tss_page.as_ptr(),
                NestedPagingStructureEntryType::R,
            ),
            vm.build_translation(
                PAGE_TABLE_4_INDEX << BASE_PAGE_SHIFT,
                page_table_4_page.as_ptr(),
                NestedPagingStructureEntryType::R,
            ),
            vm.build_translation(
                PAGE_TABLE_3_INDEX << BASE_PAGE_SHIFT,
                page_table_3_page.as_ptr(),
                NestedPagingStructureEntryType::R,
            ),
            vm.build_translation(
                CODE_ENTRY_PAGE_INDEX << BASE_PAGE_SHIFT,
                code_entry_page.as_ptr(),
                NestedPagingStructureEntryType::Rx,
            ),
        ];

        for t in translations.into_iter() {
            if let Err(err) = t {
                error!("Failed to build translation: {:?}", err);
                return Err(err);
            }
        }

        Ok(Self {
            memory_code_page: Pin::from(code_page),
            memory_code_entry_page: Pin::from(code_entry_page),
            memory_stack_page: Pin::from(stack_page),
            memory_gdt_page: Pin::from(gdt_page),
            memory_tss_page: Pin::from(tss_page),
            memory_page_table_4: Pin::from(page_table_4_page),
            memory_page_table_3: Pin::from(page_table_3_page),
            vm,
            initial_state: state,
        })
    }

    pub fn load_code_blob(&mut self, code_blob: &[u8]) {
        self.memory_code_page.fill(0x90) /* nop */;

        unsafe {
            core::ptr::copy_nonoverlapping(
                code_blob.as_ptr(),
                self.memory_code_page.as_slice_mut().as_mut_ptr(),
                code_blob.len().min(4096),
            );
        }
    }

    pub fn prepare_vm_state(&mut self) {
        // is a full vm reset required? -> YES, interrupt state or smth is transferred across runs
        self.vm.initialize().expect("it also worked the first time");
        self.vm.vt.load_state(&self.initial_state);
        self.vm.vt.set_preemption_timer(1e6 as u64);

        self.memory_stack_page.zero();
        // unsafe { (*(0x1000 as *const Page as *mut Page)).zero(); } // todo!
    }

    pub fn run_vm(&mut self, coverage_collection: bool) -> VmExitReason {
        self.switch_coverage_mode(coverage_collection);
        self.vm.vt.run()
    }

    pub fn trace_vm(&mut self, trace: &mut Trace, max_trace_length: usize) -> VmExitReason {
        self.vm.vt.enable_tracing();

        trace.clear();

        let mut eti_count = 0;

        let mut last_exit = VmExitReason::Unexpected(0);
        for _ in 0..(if max_trace_length == 0 {
            usize::MAX
        } else {
            max_trace_length
        }) {
            trace.push(self.vm.vt.registers().rip);
            last_exit = self.vm.vt.run();

            if last_exit == VmExitReason::MonitorTrap
                || last_exit == VmExitReason::ExternalInterrupt
            {
                eti_count = 0;
                continue;
            } else if last_exit == VmExitReason::ExternalInterrupt {
                eti_count += 1;
                if eti_count > 100 {
                    break;
                }
                continue;
            } else {
                break;
            }
        }

        self.vm.vt.disable_tracing();
        last_exit
    }

    pub fn state_trace_vm(
        &mut self,
        trace: &mut StateTrace<VmState>,
        max_trace_length: usize,
    ) -> VmExitReason {
        self.vm.vt.enable_tracing();

        trace.clear();

        let mut eti_count = 0;

        let mut state = self.initial_state.clone();
        trace.push(state.clone());

        let mut last_exit = VmExitReason::Unexpected(0);
        if max_trace_length == 0 {
            return last_exit;
        }
        for _ in 0..max_trace_length {
            last_exit = self.vm.vt.run();
            self.vm.vt.save_state(&mut state);
            trace.push(state.clone());

            if last_exit == VmExitReason::MonitorTrap {
                eti_count = 0;
                continue;
            } else if last_exit == VmExitReason::ExternalInterrupt {
                eti_count += 1;
                if eti_count > 100 {
                    break;
                }
                continue;
            } else {
                break;
            }
        }

        self.vm.vt.disable_tracing();
        last_exit
    }

    pub fn state_trace_vm_memory_introspection(
        &mut self,
        trace: &mut StateTrace<(VmState, Vec<MemoryAccess>)>,
        max_trace_length: usize,
    ) -> VmExitReason {
        pub const MAX_INDEX: usize = CODE_ENTRY_PAGE_INDEX;

        fn reset_permissions(vm: &mut Hypervisor) {
            let execute_permission = vm
                .vm
                .vt
                .nps_entry_flags(NestedPagingStructureEntryType::X)
                .permission;
            let no_permission = vm
                .vm
                .vt
                .nps_entry_flags(NestedPagingStructureEntryType::None)
                .permission;

            for index in 0..MAX_INDEX + 1 {
                let translation = vm.vm.get_translation(index << BASE_PAGE_SHIFT).unwrap();
                let permissions = translation.permission() as u8;

                let executable = permissions & execute_permission != 0;

                if executable {
                    translation.set_permission(execute_permission as u64);
                } else {
                    translation.set_permission(no_permission as u64);
                }
            }

            vm.vm.vt.invalidate_caches();
        }

        let page_permissions = (0..MAX_INDEX + 1)
            .map(|index| {
                self.vm
                    .get_translation(index << BASE_PAGE_SHIFT)
                    .unwrap()
                    .permission() as u8
            })
            .collect_vec();

        reset_permissions(self);

        self.vm.vt.enable_tracing();

        trace.clear();

        let mut eti_count = 0;

        let mut state = self.initial_state.clone();
        trace.push((state.clone(), Vec::new()));

        let mut last_exit = VmExitReason::Unexpected(0);
        if max_trace_length == 0 {
            return last_exit;
        }
        let mut memory_accesses = BTreeMap::new();
        for _ in 0..max_trace_length {
            last_exit = self.vm.vt.run();

            if last_exit == VmExitReason::MonitorTrap {
                eti_count = 0;

                if !memory_accesses.is_empty() {
                    reset_permissions(self);
                }

                self.vm.vt.save_state(&mut state);
                trace.push((
                    state.clone(),
                    memory_accesses.values().cloned().collect_vec(),
                ));
                memory_accesses.clear();

                continue;
            } else if last_exit == VmExitReason::ExternalInterrupt {
                eti_count += 1;
                if eti_count > 100 {
                    self.vm.vt.save_state(&mut state);
                    trace.push((
                        state.clone(),
                        memory_accesses.values().cloned().collect_vec(),
                    ));
                    break;
                }
                continue;
            } else if let VmExitReason::EPTPageFault(qualification) = &last_exit {
                let normalized_address = qualification.gpa as u64 % (1u64 << 30); // map to 1GB address space
                let normalized_page = normalized_address >> BASE_PAGE_SHIFT;
                if normalized_page > MAX_INDEX as u64 {
                    self.vm.vt.save_state(&mut state);
                    trace.push((
                        state.clone(),
                        memory_accesses.values().cloned().collect_vec(),
                    ));
                    break;
                }

                let write_permission = self
                    .vm
                    .vt
                    .nps_entry_flags(NestedPagingStructureEntryType::W)
                    .permission;

                let read_permission = self
                    .vm
                    .vt
                    .nps_entry_flags(NestedPagingStructureEntryType::R)
                    .permission;

                if !qualification.was_writable
                    && qualification.data_write
                    && *page_permissions.get(normalized_page as usize).unwrap() & write_permission
                        > 0
                {
                    let entry = memory_accesses
                        .entry(normalized_address)
                        .or_insert(MemoryAccess {
                            address: normalized_address,
                            read: false,
                            write: false,
                        });
                    entry.write = true;

                    let entry = self
                        .vm
                        .get_translation((normalized_page << BASE_PAGE_SHIFT) as usize)
                        .unwrap();
                    entry.set_permission(entry.permission() | write_permission as u64);
                    self.vm.vt.invalidate_caches();
                } else if !qualification.was_readable
                    && qualification.data_read
                    && *page_permissions.get(normalized_page as usize).unwrap() & read_permission
                        > 0
                {
                    let entry = memory_accesses
                        .entry(normalized_address)
                        .or_insert(MemoryAccess {
                            address: normalized_address,
                            read: false,
                            write: false,
                        });
                    entry.read = true;

                    let entry = self
                        .vm
                        .get_translation((normalized_page << BASE_PAGE_SHIFT) as usize)
                        .unwrap();
                    entry.set_permission(entry.permission() | read_permission as u64);
                    self.vm.vt.invalidate_caches();
                } else {
                    self.vm.vt.save_state(&mut state);
                    trace.push((
                        state.clone(),
                        memory_accesses.values().cloned().collect_vec(),
                    ));
                    break;
                }
            } else {
                self.vm.vt.save_state(&mut state);
                trace.push((
                    state.clone(),
                    memory_accesses.values().cloned().collect_vec(),
                ));
                break;
            }
        }

        self.vm.vt.disable_tracing();

        // restore permissions
        for (index, permission) in page_permissions.into_iter().enumerate() {
            let translation = self.vm.get_translation(index << BASE_PAGE_SHIFT).unwrap();
            translation.set_permission(permission as u64);
        }
        self.vm.vt.invalidate_caches();

        last_exit
    }

    pub fn run_with_callback(
        &mut self,
        coverage_collection: bool,
        after_execution: fn(),
    ) -> VmExitReason {
        self.switch_coverage_mode(coverage_collection);
        self.vm.vt.run_with_callback(after_execution)
    }

    pub fn capture_state(&self, state: &mut VmState) {
        self.vm.vt.save_state(state);
    }

    pub fn selfcheck(&mut self) -> bool {
        let mut assembler = CodeAssembler::new(64).unwrap();

        let mut start_sequence = assembler.create_label();
        let mut hlt_int3 = assembler.create_label();
        let mut label_loop = assembler.create_label();

        assembler.jmp(start_sequence).unwrap();
        assembler.set_label(&mut hlt_int3).unwrap();
        assembler.int3().unwrap();
        assembler.set_label(&mut start_sequence).unwrap();
        assembler.mov(code_asm::rax, 0x11u64).unwrap();
        assembler.push(code_asm::rax).unwrap();
        assembler.mov(code_asm::rax, 0x22u64).unwrap();
        assembler.pop(code_asm::rbx).unwrap();
        assembler.add(code_asm::rax, code_asm::rbx).unwrap();
        assembler.jmp(hlt_int3).unwrap();
        assembler.set_label(&mut label_loop).unwrap();
        assembler.jmp(label_loop).unwrap();

        let code = assembler
            .assemble(0)
            .expect("failed to assemble selfcheck code");

        crate::disassemble_code(&code);

        self.load_code_blob(&code);
        self.prepare_vm_state();

        let mut state = self.initial_state.clone();

        let result = self.vm.vt.run();
        if let VmExitReason::Exception(ExceptionQualification {
            rip,
            exception_code,
        }) = result
        {
            if exception_code != GuestException::BreakPoint {
                error!("Selfcheck: Unexpected exception code: {:?}", exception_code);
                return false;
            }
            if rip != 2 {
                error!("Selfcheck: Unexpected rip: {:x}", rip);
                return false;
            }
            self.vm.vt.save_state(&mut state);
            if state.standard_registers.rax != 0x33 {
                error!(
                    "Selfcheck: Unexpected rax: {:x}",
                    state.standard_registers.rax
                );
                return false;
            }

            return true;
        } else {
            error!("Selfcheck: Unexpected exit reason: {:x?}", result);
            return false;
        }
    }

    fn generate_code_entry(code_entry: u64, current_rip: u64) -> Vec<u8> {
        let mut assembler = CodeAssembler::new(64).unwrap();

        if cfg!(not(feature = "__debug_dont_reinitialize_fpu")) {
            assembler.finit().unwrap();
        }
        assembler.wbinvd().unwrap();
        assembler.mfence().unwrap();
        assembler.lfence().unwrap();

        assembler.mov(code_asm::rax, code_entry).unwrap();
        assembler.jmp(code_asm::rax).unwrap();

        assembler
            .assemble(current_rip)
            .expect("failed to assemble code entry")
    }

    pub fn switch_coverage_mode(&mut self, coverage_collection: bool) {
        let rw = self
            .vm
            .vt
            .nps_entry_flags(NestedPagingStructureEntryType::Rw);
        let none = self
            .vm
            .vt
            .nps_entry_flags(NestedPagingStructureEntryType::None);

        let translation = match self
            .vm
            .get_translation(COVERAGE_PAGE_INDEX << BASE_PAGE_SHIFT)
        {
            Ok(x) => x,
            Err(err) => {
                error!("Error while getting translation: {err:?}");
                return;
            }
        };

        if translation.permission() as u8 == rw.permission && !coverage_collection {
            translation.set_permission(none.permission as u64);
            self.vm.vt.invalidate_caches();
        } else if translation.permission() as u8 == none.permission && coverage_collection {
            translation.set_permission(rw.permission as u64);
            self.vm.vt.invalidate_caches();
        }
    }
}

#[cfg_attr(feature = "__debug_performance_trace", track_time)]
impl Drop for Hypervisor {
    fn drop(&mut self) {
        self.vm.vt.disable();
    }
}
