//! Virtual Machine Management Module
//!
//! This module provides the core functionality for managing virtual machines
//! in the hypervisor. It handles VM creation, memory management through nested
//! paging, and hardware-assisted virtualization.

use crate::error::HypervisorError;
use crate::{
    hardware_vt::{
        vmx::Vmx, HardwareVt, NestedPagingStructure, NestedPagingStructureEntry,
        NestedPagingStructureEntryType,
    },
    Page,
};
use alloc::boxed::Box;
use core::ptr::addr_of;
use log::trace;

/// Represents a virtual machine in the hypervisor
///
/// This structure encapsulates all the necessary components to manage a virtual
/// machine.
#[derive(derivative::Derivative)]
#[derivative(Debug)]
pub struct Vm {
    /// Hardware virtualization implementation (VT-x or SVM)
    ///
    /// This field encapsulates the implementation of hardware-assisted
    /// virtualization technology.
    pub vt: Box<dyn HardwareVt>,

    /// The nested PML4 (Page Map Level 4) table
    ///
    /// This is the root of the nested paging hierarchy. All other nested paging
    /// structures are built dynamically at runtime.
    #[derivative(Debug = "ignore")]
    nested_pml4: Box<NestedPagingStructure>,

    /// Preallocated nested paging structures
    ///
    /// A pool of preallocated nested paging structures used for dynamically
    /// building GPA (Guest Physical Address) to PA (Physical Address) translations.
    #[derivative(Debug = "ignore")]
    nested_paging_structures: Box<[NestedPagingStructure]>,

    /// Count of used nested paging structures
    ///
    /// Tracks how many structures from `nested_paging_structures` have been
    /// consumed for building the page tables.
    used_nps_count: usize,
}

impl Vm {
    /// Creates a new virtual machine instance
    pub fn new() -> Self {
        // The number of pre-allocated nested paging structures. The more memory the VM
        // accesses, the more tables we need. If the VM attempts to access more
        // memory than this can manage, the hypervisor will panic.
        const NPS_COUNT: usize = 1024;

        // Use VMX on Intel and SMV on AMD.
        let vt: Box<dyn HardwareVt> = if is_intel() {
            trace!("Processor is Intel");
            Box::new(Vmx::new())
        } else {
            trace!("Processor is AMD");
            // Box::new(Svm::new())
            todo!()
        };

        let nested_pml4 = unsafe { Box::<NestedPagingStructure>::new_zeroed().assume_init() };

        let nested_paging_structures =
            unsafe { Box::<[NestedPagingStructure]>::new_zeroed_slice(NPS_COUNT).assume_init() };

        Self {
            vt,
            nested_pml4,
            nested_paging_structures,
            used_nps_count: 0,
        }
    }

    /// Returns the address of the nested PML4 table
    pub(crate) fn nested_pml4_addr(&mut self) -> *mut NestedPagingStructure {
        core::ptr::from_mut(self.nested_pml4.as_mut())
    }

    /// Gets the translation entry for a guest physical address
    ///
    /// This function walks through the nested paging hierarchy (PML4 -> PDPT -> PD -> PT)
    /// to find or create the appropriate page table entry for the given GPA.
    ///
    /// # Arguments
    ///
    /// * `gpa` - The guest physical address to translate
    ///
    /// # Returns
    ///
    /// A mutable reference to the page table entry that will handle the translation, or Err if out of memory.
    pub fn get_translation(
        &mut self,
        gpa: usize,
    ) -> crate::Result<&mut NestedPagingStructureEntry> {
        let pml4i = (gpa >> 39) & 0b1_1111_1111;
        let pdpti = (gpa >> 30) & 0b1_1111_1111;
        let pdi = (gpa >> 21) & 0b1_1111_1111;
        let pti = (gpa >> 12) & 0b1_1111_1111;

        // Locate PML4, index it, build PML4e as needed
        /*
                                Nested PML4 (4KB)
            nested_pml4_addr -> +-----------+
                                |           | [0]
                                +-----------+
                                |   pml4e   | [1] <----- pml4i (eg = 1)
                                +-----------+
                                |           |
                                     ...
                                |           | [512]
                                +-----------+
            walk_table() does two things if the indexed entry is empty:
                1. allocate a new table (ie, next table) from preallocated buffer
                2. update the entry's pfn to point to the next table (see below diagram)

            pml4e (64bit)
            +-----------------+------+
            |    N:12 (pfn)   | 11:0 |
            +-----------------+------+
                   \                Nested PDPT (4KB)
                    \-------------> +-----------+
                                    |           | [0]
                                    +-----------+
                                    |   pdpte   | [1] <----- pdpti (eg = 1)
                                    +-----------+
                                    |           |
                                        ...
                                    |           | [512]
                                    +-----------+
        */
        let pml4 = unsafe { self.nested_pml4_addr().as_mut() }.unwrap();
        let pml4e = self.walk_table(pml4, pml4i)?;

        // Locate PDPT, index it, build PDPTe as needed
        let pdpt = pml4e.next_table_mut();
        let pdpte = self.walk_table(pdpt, pdpti)?;

        // Locate PD, index it, build PDe as needed
        let pd = pdpte.next_table_mut();
        let pde = self.walk_table(pd, pdi)?;

        // Locate PT, index it, build PTe as needed
        let pt = pde.next_table_mut();
        let pte = &mut pt.entries[pti];
        // assert_eq!(pte.0, 0);

        Ok(pte)
    }

    /// Builds a translation mapping for a guest physical address
    ///
    /// This function creates or updates the page table entries to map a guest
    /// physical address to a physical address with the specified entry type.
    ///
    /// # Arguments
    ///
    /// * `gpa` - The guest physical address to map
    /// * `pa` - The physical address to map to
    /// * `entry_type` - The type of page table entry to create
    #[allow(clippy::similar_names)]
    pub fn build_translation(
        &mut self,
        gpa: usize,
        pa: *const Page,
        entry_type: NestedPagingStructureEntryType,
    ) -> crate::Result<()> {
        // Make it non-writable so that copy-on-write is done for dirty pages.
        let flags = self.vt.nps_entry_flags(entry_type);

        let pte = self.get_translation(gpa)?;
        pte.set_translation(pa as u64, flags);

        Ok(())
    }

    /// Walks a nested paging structure to find or create an entry
    ///
    /// This function handles the creation and initialization of nested paging
    /// structure entries. If an entry doesn't exist, it allocates a new table
    /// from the preallocated pool and initializes the entry.
    ///
    /// # Returns
    ///
    /// A mutable reference to the requested page table entry, or Err if out of memory.
    fn walk_table<'a>(
        &mut self,
        table: &'a mut NestedPagingStructure,
        index: usize,
    ) -> crate::Result<&'a mut NestedPagingStructureEntry> {
        let entry = &mut table.entries[index];

        // If there is no information about the next table in the entry, add that.
        // An unused `nested_paging_structures` is used as a next table.
        if entry.0 == 0 {
            if self.used_nps_count >= self.nested_paging_structures.len() {
                return Err(HypervisorError::NestedPagingStructuresExhausted);
            }
            let next_table = addr_of!(self.nested_paging_structures[self.used_nps_count]) as u64;
            entry.set_translation(
                next_table,
                self.vt.nps_entry_flags(NestedPagingStructureEntryType::Rwx),
            );
            self.used_nps_count += 1;
        }
        Ok(entry)
    }

    /// Initializes the virtual machine
    ///
    /// This function sets up the hardware virtualization environment by
    /// providing the address of the nested PML4 table to the hardware
    /// virtualization implementation.
    pub fn initialize(&mut self) -> Result<(), &'static str> {
        let addr = self.nested_pml4_addr() as u64;
        self.vt.initialize(addr)
    }
}

/// Checks if the current processor is an Intel processor
///
/// This function uses CPUID to determine if the processor is manufactured
/// by Intel, which affects the choice of hardware virtualization technology
/// (VT-x for Intel, SVM for AMD).
fn is_intel() -> bool {
    x86::cpuid::CpuId::new().get_vendor_info().unwrap().as_str() == "GenuineIntel"
}
