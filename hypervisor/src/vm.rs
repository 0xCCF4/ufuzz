//! The module containing the [`Vm`] type.

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

/// The representation of a virtual machine, made up of collection of registers,
/// which is managed through [`HardwareVt`], preallocated
/// [`NestedPagingStructure`]s to build GPA -> PA translations, and preallocated
/// dirty [`Page`]s to back GPAs that are modified by the VM.
#[derive(derivative::Derivative)]
#[derivative(Debug)]
pub struct Vm {
    /// Encapsulates implementation of hardware assisted virtualization
    /// technology, which is capable of managing VM's registers and memory.
    pub vt: Box<dyn HardwareVt>,

    /// The nested PML4. All other nested paging structures are built on the fly
    /// by consuming [`Vm::nested_paging_structures`].
    #[derivative(Debug = "ignore")]
    nested_pml4: Box<NestedPagingStructure>,

    /// Preallocated nested paging structures for dynamically building GPA -> PA
    /// translation.
    #[derivative(Debug = "ignore")]
    nested_paging_structures: Box<[NestedPagingStructure]>,

    /// How many [`Vm::nested_paging_structures`] has been consumed.
    used_nps_count: usize,
}

impl Vm {
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

    pub(crate) fn nested_pml4_addr(&mut self) -> *mut NestedPagingStructure {
        core::ptr::from_mut(self.nested_pml4.as_mut())
    }

    /// Builds nested paging translation for `gpa` to translate to `pa`.
    ///
    /// This function does so by walking through whole PML4 -> PDPT -> PD -> PT
    /// as a processor does, and allocating tables and initializing table
    /// entries as needed.
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
        assert_eq!(pte.0, 0);

        Ok(pte)
    }

    /// Builds nested paging translation for `gpa` to translate to `pa`.
    ///
    /// This function does so by walking through whole PML4 -> PDPT -> PD -> PT
    /// as a processor does, and allocating tables and initializing table
    /// entries as needed.
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

    /// Locates a nested paging structure entry from `table` using `index`.
    ///
    /// This function initializes the entry if it is not yet. `table` must be
    /// either a nested PML4, PDPT, or PD. Not PT.
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

    pub fn initialize(&mut self) -> Result<(), &'static str> {
        let addr = self.nested_pml4_addr() as u64;
        self.vt.initialize(addr)
    }
}

/// Checks whether the current processor is Intel-processors (as opposed to
/// AMD).
fn is_intel() -> bool {
    x86::cpuid::CpuId::new().get_vendor_info().unwrap().as_str() == "GenuineIntel"
}
