use crate::coverage_collector;
use crate::interface::safe::ComInterface;
#[cfg(feature = "no_std")]
use alloc::vec::Vec;
use custom_processing_unit::{
    apply_patch, call_custom_ucode_function, disable_all_hooks, enable_hooks, lmfence,
    restore_hooks, CustomProcessingUnit,
};
use data_types::addresses::{Address, UCInstructionAddress};
use ucode_compiler::utils::sequence_word::SequenceWord;

const COVERAGE_ENTRIES: usize = UCInstructionAddress::MAX.to_const();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverageError {
    TooManyHooks,
    SetupFailed,
    AddressNotHookable(UCInstructionAddress),
}

pub struct CoverageHarness<'a, 'b, 'c> {
    interface: &'a mut ComInterface<'b>,
    coverage: [u8; COVERAGE_ENTRIES], // every forth entry, beginning at 3 is zero
    previous_hook_settings: usize,
    custom_processing_unit: &'c CustomProcessingUnit,
}

impl<'a, 'b, 'c> CoverageHarness<'a, 'b, 'c> {
    pub fn new(
        interface: &'a mut ComInterface<'b>,
        cpu: &'c CustomProcessingUnit,
    ) -> CoverageHarness<'a, 'b, 'c> {
        CoverageHarness {
            interface,
            coverage: [0; COVERAGE_ENTRIES],
            previous_hook_settings: 0,
            custom_processing_unit: cpu,
        }
    }

    pub fn init(&mut self) {
        apply_patch(&coverage_collector::PATCH);
        self.interface.zero_jump_table();
        self.previous_hook_settings = disable_all_hooks();
    }

    pub fn reset_coverage(&mut self) {
        self.coverage = [0; COVERAGE_ENTRIES];
    }

    #[inline(always)]
    fn pre_execution(&mut self, hooks: &[UCInstructionAddress]) -> Result<(), CoverageError> {
        self.interface.reset_coverage();

        if hooks.len() > self.interface.description().max_number_of_hooks as usize {
            return Err(CoverageError::TooManyHooks);
        }

        self.interface.write_jump_table_all(hooks);

        let result =
            call_custom_ucode_function(coverage_collector::LABEL_FUNC_SETUP, [hooks.len(), 0, 0]);

        if result.rax != 0x664200006642 {
            return Err(CoverageError::SetupFailed);
        }

        Ok(())
    }

    #[inline(always)]
    fn post_execution(&mut self, hooks: &[UCInstructionAddress]) {
        for (index, address) in hooks.iter().enumerate() {
            let covered = self.interface.read_coverage_table(index) > 0;
            if covered {
                self.coverage[address.address()] += 1;
            }
        }
    }

    /*
    pub fn execute_multiple_times<T, F: FnMut(usize, &mut T)>(
        &mut self,
        hooks: &[UCInstructionAddress],
        mut func: F,
        param: &mut T,
        number_of_times: usize,
    ) -> Result<(), &'static str> {
        self.pre_execution(hooks)?;
        enable_hooks();

        lmfence();
        for i in 0..number_of_times {
            core::hint::black_box(&mut func)(i, param);
            lmfence();
        }

        disable_all_hooks();
        self.post_execution(hooks);

        Ok(())
    }*/

    pub fn execute<T, R, F: FnOnce(T) -> R>(
        &mut self,
        hooks: &[UCInstructionAddress],
        func: F,
        param: T,
    ) -> Result<R, CoverageError> {
        self.pre_execution(hooks)?;
        enable_hooks();

        lmfence();
        let result = core::hint::black_box(func)(param);
        lmfence();

        disable_all_hooks();
        self.post_execution(hooks);

        Ok(result)
    }

    pub fn covered<A: AsRef<UCInstructionAddress>>(&self, address: A) -> bool {
        self.coverage[address.as_ref().address()] > 0
    }

    pub fn get_coverage(&self) -> &[u8; COVERAGE_ENTRIES] {
        &self.coverage
    }

    pub fn is_hookable(&self, address: UCInstructionAddress) -> bool {
        if address >= UCInstructionAddress::MSRAM_START {
            return false;
        }

        if address.address() % 4 != 0 && address.address() % 4 != 2 {
            return false;
        }

        true // todo
    }

    fn compute_modified_sequence_word(
        &self,
        hooked_address: UCInstructionAddress,
        sequence_word: SequenceWord,
    ) -> Result<SequenceWord, CoverageError> {
        todo!()
    }
}

impl<'a, 'b, 'c> Drop for CoverageHarness<'a, 'b, 'c> {
    fn drop(&mut self) {
        let mut v = Vec::with_capacity(self.interface.description().max_number_of_hooks as usize);

        for _ in 0..self.interface.description().max_number_of_hooks {
            v.push(UCInstructionAddress::ZERO);
        }

        // zero hooks
        let _ = self.pre_execution(v.as_ref());

        restore_hooks(self.previous_hook_settings);
    }
}
