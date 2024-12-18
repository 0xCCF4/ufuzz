mod modification_engine;

use crate::coverage_collector;
use crate::coverage_harness::modification_engine::{ModificationEngineSettings, NotHookableReason};
use crate::interface::safe::ComInterface;
use crate::interface_definition::{CoverageEntry, InstructionTableEntry};
#[cfg(feature = "no_std")]
use alloc::vec::Vec;
use core::fmt::Debug;
use custom_processing_unit::{
    apply_patch, call_custom_ucode_function, disable_all_hooks, enable_hooks, lmfence,
    CustomProcessingUnit, FunctionResult, HookGuard,
};
use data_types::addresses::UCInstructionAddress;
use ucode_compiler::utils::instruction::Instruction;
use ucode_compiler::utils::sequence_word::{DisassembleError, SequenceWord};
use ucode_compiler::utils::Triad;
// const COVERAGE_ENTRIES: usize = UCInstructionAddress::MAX.to_const();

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoverageError {
    TooManyHooks,
    SetupFailed(FunctionResult),
    AddressNotHookable(UCInstructionAddress, NotHookableReason),
    SequenceWordDissembleError(DisassembleError),
}

pub struct CoverageHarness<'a, 'b, 'c> {
    interface: &'a mut ComInterface<'b>,
    // coverage: [CoverageEntry; COVERAGE_ENTRIES], // every forth entry, beginning at 3 is zero
    previous_hook_settings: Option<HookGuard>,
    custom_processing_unit: &'c CustomProcessingUnit,
    compile_mode: ModificationEngineSettings,
}

impl<'a, 'b, 'c> CoverageHarness<'a, 'b, 'c> {
    pub fn new(
        interface: &'a mut ComInterface<'b>,
        cpu: &'c CustomProcessingUnit,
    ) -> CoverageHarness<'a, 'b, 'c> {
        apply_patch(&coverage_collector::PATCH);
        interface.zero_jump_table();
        interface.zero_clock_table();
        interface.zero_clock_table_settings();

        CoverageHarness {
            interface,
            // coverage: [0; COVERAGE_ENTRIES],
            previous_hook_settings: Some(HookGuard::disable_all()),
            custom_processing_unit: cpu,
            compile_mode: ModificationEngineSettings::empty(),
        }
    }

    #[inline(always)]
    fn pre_execution(&mut self, hooks: &[UCInstructionAddress]) -> Result<(), CoverageError> {
        if hooks.len() > self.interface.description().max_number_of_hooks as usize {
            return Err(CoverageError::TooManyHooks);
        }

        let mut instructions = Vec::with_capacity(hooks.len());
        for hook in hooks.iter().copied() {
            if hooks
                .iter()
                .filter(|a| a.triad_base() == hook.triad_base())
                .count()
                > 1
            {
                return Err(CoverageError::AddressNotHookable(
                    hook,
                    NotHookableReason::TriadAlreadyHooked,
                ));
            }

            if let Err(err) = self.is_hookable(hook) {
                return Err(CoverageError::AddressNotHookable(hook, err));
            }

            let triads = self
                .modify_triad_for_hooking(hook, &self.compile_mode)
                .map_err(|err| CoverageError::AddressNotHookable(hook, err))?;

            for triad in triads.into_iter() {
                instructions.push(triad);
            }
        }

        self.interface.write_jump_table_all(hooks);
        self.interface.write_instruction_table_all(instructions);
        self.interface.reset_coverage();

        let result =
            call_custom_ucode_function(coverage_collector::LABEL_FUNC_SETUP, [hooks.len(), 0, 0]);

        if result.rax != 0x664200006642 {
            return Err(CoverageError::SetupFailed(result));
        }

        Ok(())
    }

    #[inline(always)]
    fn post_execution(&mut self, hooks: &[UCInstructionAddress]) -> Vec<ExecutionResultEntry> {
        let mut result = Vec::with_capacity(hooks.len());
        for (index, address) in hooks.iter().enumerate() {
            let address = address.align_even();

            let covered_even = self.interface.read_coverage_table(index << 1);
            let covered_odd = self.interface.read_coverage_table((index << 1) + 1);

            result.push(if covered_even > 0 {
                ExecutionResultEntry::Covered {
                    address,
                    count: covered_even,
                }
            } else {
                ExecutionResultEntry::NotCovered { address }
            });

            result.push(if covered_odd > 0 {
                ExecutionResultEntry::Covered {
                    address: address + 1,
                    count: covered_odd,
                }
            } else {
                ExecutionResultEntry::NotCovered {
                    address: address + 1,
                }
            });
        }
        result
    }

    pub fn setup(&mut self, hooks: &[UCInstructionAddress]) -> Result<(), CoverageError> {
        self.pre_execution(&hooks)
    }

    pub fn execute<T, R, F: FnOnce(T) -> R>(
        &mut self,
        hooks: &[UCInstructionAddress],
        func: F,
        param: T,
    ) -> Result<ExecutionResult<R>, CoverageError> {
        self.pre_execution(hooks)?;
        enable_hooks();

        lmfence();
        let result = core::hint::black_box(func)(param);
        lmfence();

        disable_all_hooks();
        let timing = self.post_execution(hooks);

        Ok(ExecutionResult {
            result,
            hooks: timing,
        })
    }

    pub fn is_hookable(&self, address: UCInstructionAddress) -> Result<(), NotHookableReason> {
        modification_engine::is_hookable(address, self.custom_processing_unit.rom(), &self.compile_mode)
    }

    fn modify_triad_for_hooking(
        &self,
        hooked_address: UCInstructionAddress,
        mode: &ModificationEngineSettings
    ) -> Result<[InstructionTableEntry; 4], NotHookableReason> {
        let even = modification_engine::modify_triad_for_hooking(
            hooked_address.align_even(),
            self.custom_processing_unit.rom(),
            mode
        )?;
        let odd = modification_engine::modify_triad_for_hooking(
            hooked_address.align_even() + 1,
            self.custom_processing_unit.rom(),
            mode
        )?;
        Ok([
            even[0]
                .assemble()
                .map_err(NotHookableReason::ModificationFailedSequenceWordBuild)?,
            even[1]
                .assemble()
                .map_err(NotHookableReason::ModificationFailedSequenceWordBuild)?,
            odd[0]
                .assemble()
                .map_err(NotHookableReason::ModificationFailedSequenceWordBuild)?,
            odd[1]
                .assemble()
                .map_err(NotHookableReason::ModificationFailedSequenceWordBuild)?,
        ])
    }
}

impl<'a, 'b, 'c> Drop for CoverageHarness<'a, 'b, 'c> {
    fn drop(&mut self) {
        for i in 0..self.interface.description().max_number_of_hooks {
            let address = UCInstructionAddress::ZERO;
            let triads = [Triad {
                instructions: [Instruction::NOP; 3],
                sequence_word: SequenceWord::new()
                    .apply_goto(0, coverage_collector::LABEL_EXIT_TRAP),
            }; 4];
            for triad in triads.iter().enumerate() {
                self.interface
                    .write_instruction_table(i as usize * 4 + triad.0, triad.1.assemble().unwrap())
            }
            self.interface.write_jump_table(i as usize, address);
        }

        self.interface.reset_coverage();

        let _ = call_custom_ucode_function(
            coverage_collector::LABEL_FUNC_SETUP,
            [
                self.interface.description().max_number_of_hooks as usize,
                0,
                0,
            ],
        );

        // dropping the hook guard will restore the hooks
        drop(self.previous_hook_settings.take());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionResultEntry {
    NotCovered {
        address: UCInstructionAddress,
    },
    Covered {
        address: UCInstructionAddress,
        count: CoverageEntry,
    },
}

impl ExecutionResultEntry {
    pub fn address(&self) -> UCInstructionAddress {
        match self {
            ExecutionResultEntry::Covered { address, .. } => *address,
            ExecutionResultEntry::NotCovered { address } => *address,
        }
    }

    pub fn coverage(&self) -> CoverageEntry {
        match self {
            ExecutionResultEntry::Covered { count, .. } => *count,
            ExecutionResultEntry::NotCovered { .. } => 0,
        }
    }

    pub fn covered(&self) -> bool {
        self.coverage() > 0
    }
}

pub struct ExecutionResult<R> {
    pub result: R,
    pub hooks: Vec<ExecutionResultEntry>,
}

impl<R: Clone> Clone for ExecutionResult<R> {
    fn clone(&self) -> Self {
        ExecutionResult {
            result: self.result.clone(),
            hooks: self.hooks.clone(),
        }
    }
}

impl<R: Debug> Debug for ExecutionResult<R> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ExecutionResult")
            .field("result", &self.result)
            .field("hooks", &self.hooks)
            .finish()
    }
}
