use crate::coverage_collector;
use crate::interface::safe::ComInterface;
use custom_processing_unit::{apply_patch, disable_all_hooks, enable_hooks, hook, lmfence};
use data_types::addresses::{Address, MSRAMHookIndex, UCInstructionAddress};

const COVERAGE_ENTRIES: usize = UCInstructionAddress::MAX.to_const();

pub struct CoverageHarness<'a, 'b> {
    interface: &'a mut ComInterface<'b>,
    coverage: [u8; COVERAGE_ENTRIES], // every forth entry, beginning at 3 is zero
    last_number_of_hooks: usize,
}

impl<'a, 'b> CoverageHarness<'a, 'b> {
    pub fn new(interface: &'a mut ComInterface<'b>) -> CoverageHarness<'a, 'b> {
        CoverageHarness {
            interface,
            coverage: [0; COVERAGE_ENTRIES],
            last_number_of_hooks: 0,
        }
    }

    fn init(&mut self) {
        apply_patch(&coverage_collector::PATCH);
        self.interface.zero_jump_table();
    }

    #[inline(always)]
    fn pre_execution(&mut self, hooks: &[UCInstructionAddress]) -> Result<(), &'static str> {
        self.interface.reset_coverage();

        if hooks.len() > self.interface.description().max_number_of_hooks {
            return Err("Requested too many hooks");
        }

        for (index, address) in hooks.iter().enumerate() {
            if hook(
                coverage_collector::LABEL_FUNC_HOOK,
                MSRAMHookIndex::ZERO + index,
                *address,
                coverage_collector::LABEL_HOOK_ENTRY_00 + index * 4, // todo: check
                true,
            )
            .is_err()
            {
                return Err("failed to apply hook");
            }
        }

        self.last_number_of_hooks = self.last_number_of_hooks.max(hooks.len());

        // todo: is this necessary?
        for index in hooks.len()..self.last_number_of_hooks {
            if hook(
                coverage_collector::LABEL_FUNC_HOOK,
                MSRAMHookIndex::ZERO + index,
                UCInstructionAddress::ZERO,
                UCInstructionAddress::ZERO,
                false,
            )
            .is_err()
            {
                return Err("failed to remove hook");
            }
        }

        self.last_number_of_hooks = hooks.len();

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

    pub fn execute<T, R, F: FnOnce(T) -> R>(
        &mut self,
        hooks: &[UCInstructionAddress],
        func: F,
        param: T,
    ) -> Result<R, &'static str> {
        self.pre_execution(hooks)?;

        lmfence();
        let result = func(param);
        lmfence();

        disable_all_hooks();
        self.post_execution(hooks);
        enable_hooks();

        Ok(result)
    }
}
