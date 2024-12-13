mod modification_engine;

use crate::coverage_collector;
use crate::interface::safe::ComInterface;
use crate::interface_definition::{ClockTableSettingsEntry, CoverageEntry, InstructionTableEntry};
#[cfg(feature = "no_std")]
use alloc::vec::Vec;
use core::fmt::Debug;
use custom_processing_unit::{apply_patch, call_custom_ucode_function, disable_all_hooks, enable_hooks, lmfence, read_unwrap_ucode_clock, unwrap_ucode_clock, CustomProcessingUnit, FunctionResult, HookGuard};
use data_types::addresses::{UCInstructionAddress};
use itertools::Itertools;
use ucode_compiler::utils::sequence_word::{DisassembleError};
use crate::coverage_harness::modification_engine::NotHookableReason;
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
        }
    }

    #[inline(always)]
    fn pre_execution(&mut self, hooks: &[(UCInstructionAddress, ClockTableSettingsEntry)]) -> Result<(), CoverageError> {
        if hooks.len() > self.interface.description().max_number_of_hooks as usize {
            return Err(CoverageError::TooManyHooks);
        }

        let mut instructions = Vec::with_capacity(hooks.len());
        for (hook, _) in hooks {
            if let Err(err) = self.is_hookable(*hook) {
                return Err(CoverageError::AddressNotHookable(*hook, err));
            }

            instructions.push(
                self.modify_triad_for_hooking(*hook)
                    .map_err(|err| CoverageError::AddressNotHookable(*hook, err))?,
            );
        }

        self.interface.write_jump_table_all(hooks.iter().map(|(v, _)| *v));
        self.interface.write_clock_table_settings_all(hooks.iter().map(|(_, v)| *v));
        self.interface.write_instruction_table_all(instructions);
        self.interface.zero_clock_table();
        self.interface.reset_coverage();

        let result =
            call_custom_ucode_function(coverage_collector::LABEL_FUNC_SETUP, [hooks.len(), 0, 0]);

        if result.rax != 0x664200006642 {
            return Err(CoverageError::SetupFailed(result));
        }

        Ok(())
    }

    #[inline(always)]
    fn post_execution(&mut self, hooks: &[(UCInstructionAddress, ClockTableSettingsEntry)], start_time: u64) -> Vec<ExecutionResultEntry> {
        let mut result = Vec::with_capacity(hooks.len());
        for (index, (_address, _)) in hooks.iter().enumerate() {
            let covered = self.interface.read_coverage_table(index);
            // self.coverage[address.address()] += covered;

            result.push(if covered > 0 {
                ExecutionResultEntry::Covered {
                    count: covered,
                    timing: unwrap_ucode_clock(self.interface.read_clock_table(index)).wrapping_sub(start_time),
                }
            } else {
                ExecutionResultEntry::NotCovered
            });
        }
        result
    }

    pub fn setup(&mut self, hooks: &[UCInstructionAddress]) -> Result<(), CoverageError> {
        let hooks = hooks.iter().map(|v| (*v, 0)).collect_vec();
        self.pre_execution(&hooks)
    }

    pub fn setup_with_timing_measurement(&mut self, hooks: &[(UCInstructionAddress, ClockTableSettingsEntry)]) -> Result<(), CoverageError> {
        self.pre_execution(hooks)
    }

    pub fn execute<T, R, F: FnOnce(T) -> R>(
        &mut self,
        hooks: &[UCInstructionAddress],
        func: F,
        param: T,
    ) -> Result<ExecutionResult<R>, CoverageError> {
        let hooks = hooks.iter().map(|v| (*v, 0)).collect_vec();

        self.execute_with_timing_measurement(hooks.as_ref(), func, param)
    }

    pub fn execute_with_timing_measurement<T, R, F: FnOnce(T) -> R>(
        &mut self,
        hooks: &[(UCInstructionAddress, ClockTableSettingsEntry)],
        func: F,
        param: T,
    ) -> Result<ExecutionResult<R>, CoverageError> {
        self.pre_execution(hooks)?;
        let start_time = read_unwrap_ucode_clock();
        enable_hooks();

        lmfence();
        let result = core::hint::black_box(func)(param);
        lmfence();

        disable_all_hooks();
        let timing = self.post_execution(hooks, start_time);

        Ok(ExecutionResult {
            result,
            hooks: timing,
        })
    }

    /* pub fn covered<A: AsRef<UCInstructionAddress>>(&self, address: A) -> bool {
        self.coverage[address.as_ref().address()] > 0
    } */

    /* pub fn get_coverage(&self) -> &[CoverageEntry; COVERAGE_ENTRIES] {
        &self.coverage
    } */

    pub fn is_hookable(&self, address: UCInstructionAddress) -> Result<(), NotHookableReason> {
        modification_engine::is_hookable(address, self.custom_processing_unit.rom())
    }

    fn modify_triad_for_hooking(
        &self,
        hooked_address: UCInstructionAddress,
    ) -> Result<InstructionTableEntry, NotHookableReason> {
        let result = modification_engine::modify_triad_for_hooking(hooked_address, self.custom_processing_unit.rom())?;
        Ok([
            result.instructions[0],
            result.instructions[1],
            result.instructions[2],
            result.sequence_word as u64,
        ])
    }
}

impl<'a, 'b, 'c> Drop for CoverageHarness<'a, 'b, 'c> {
    fn drop(&mut self) {
        let mut v = Vec::with_capacity(self.interface.description().max_number_of_hooks as usize);

        for _ in 0..self.interface.description().max_number_of_hooks {
            v.push((UCInstructionAddress::ZERO, 0));
        }

        // zero hooks
        let _ = self.pre_execution(v.as_ref());

        // dropping the hook guard will restore the hooks
        drop(self.previous_hook_settings.take());
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionResultEntry {
    NotCovered,
    Covered {
        count: CoverageEntry,
        timing: u64,
    },
}

impl ExecutionResultEntry {
    pub fn coverage(&self) -> CoverageEntry {
        match self {
            ExecutionResultEntry::Covered { count, .. } => *count,
            ExecutionResultEntry::NotCovered => 0,
        }
    }

    pub fn covered(&self) -> bool {
        self.coverage() > 0
    }
}

pub struct ExecutionResult<R> {
    pub result: R,
    pub hooks: Vec<ExecutionResultEntry>
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
