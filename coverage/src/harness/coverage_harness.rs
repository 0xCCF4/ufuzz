pub mod hookable_address_iterator;
pub mod modification_engine;

#[cfg(feature = "ucode")]
use crate::coverage_collector;
#[cfg(feature = "ucode")]
use crate::harness::coverage_harness::modification_engine::{
    ModificationEngineSettings, NotHookableReason,
};
#[cfg(feature = "ucode")]
use crate::interface::safe::ComInterface;
use crate::interface_definition::CoverageCount;
#[cfg(feature = "ucode")]
use crate::interface_definition::InstructionTableEntry;
#[cfg(feature = "nostd")]
use alloc::vec::Vec;
use core::fmt::Debug;
#[cfg(feature = "ucode")]
use custom_processing_unit::{
    apply_patch, call_custom_ucode_function, disable_all_hooks, enable_hooks, lmfence,
    CustomProcessingUnit, FunctionResult, HookGuard, PatchError,
};
use data_types::addresses::UCInstructionAddress;
#[cfg(feature = "timing_measurement")]
use performance_timing::TimeMeasurement;
#[cfg(feature = "ucode")]
use ucode_compiler_dynamic::sequence_word::DisassembleError;
#[cfg(feature = "ucode")]
use ucode_compiler_dynamic::Triad;
// const COVERAGE_ENTRIES: usize = UCInstructionAddress::MAX.to_const();

#[cfg(feature = "ucode")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoverageError {
    TooManyHooks,
    SetupFailed(FunctionResult),
    AddressNotHookable(UCInstructionAddress, NotHookableReason),
    SequenceWordDissembleError(DisassembleError),
}

#[cfg(feature = "ucode")]
pub struct CoverageHarness<'a, 'b, 'c> {
    interface: &'a mut ComInterface<'b>,
    // coverage: [CoverageEntry; COVERAGE_ENTRIES], // every forth entry, beginning at 3 is zero
    previous_hook_settings: Option<HookGuard>,
    custom_processing_unit: &'c CustomProcessingUnit,
    compile_mode: ModificationEngineSettings,
}

#[cfg(feature = "ucode")]
impl<'a, 'b, 'c> CoverageHarness<'a, 'b, 'c> {
    pub fn new(
        interface: &'a mut ComInterface<'b>,
        cpu: &'c CustomProcessingUnit,
    ) -> Result<CoverageHarness<'a, 'b, 'c>, PatchError> {
        apply_patch(&coverage_collector::PATCH)?;
        interface.zero_jump_table();

        Ok(CoverageHarness {
            interface,
            // coverage: [0; COVERAGE_ENTRIES],
            previous_hook_settings: Some(HookGuard::disable_all()),
            custom_processing_unit: cpu,
            compile_mode: ModificationEngineSettings::default(),
        })
    }

    #[inline(always)]
    fn pre_execution(&mut self, hooks: &[UCInstructionAddress]) -> Result<(), CoverageError> {
        #[cfg(feature = "timing_measurement")]
        let timing = if performance_timing::is_available() {
            Some(TimeMeasurement::begin("coverage::collection::prepare"))
        } else {
            None
        };

        if hooks.len() > self.interface.description().max_number_of_hooks {
            return Err(CoverageError::TooManyHooks);
        }

        let mut instructions: Vec<InstructionTableEntry> = Vec::with_capacity(hooks.len());
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

            let triad = self
                .modify_triad_for_hooking(hook, &self.compile_mode)
                .map_err(|err| CoverageError::AddressNotHookable(hook, err))?;

            let mut assembled_triad = triad.into_iter().map(|triad| {
                triad.assemble().map_err(|err| {
                    CoverageError::AddressNotHookable(
                        hook,
                        NotHookableReason::ModificationFailedSequenceWordBuild(err),
                    )
                })
            });

            instructions.push([
                assembled_triad.next().unwrap()?,
                assembled_triad.next().unwrap()?,
            ]);
        }

        self.interface.write_jump_table_all(hooks);
        self.interface.write_instruction_table_all(instructions);
        self.interface.reset_coverage();

        let result =
            call_custom_ucode_function(coverage_collector::LABEL_FUNC_SETUP, [hooks.len(), 0, 0]);

        if result.rax != 0x664200006642 {
            return Err(CoverageError::SetupFailed(result));
        }

        #[cfg(feature = "timing_measurement")]
        drop(timing);
        Ok(())
    }

    #[inline(always)]
    fn post_execution(&mut self, hooks: &[UCInstructionAddress]) -> Vec<ExecutionResultEntry> {
        #[cfg(feature = "timing_measurement")]
        let timing = if performance_timing::is_available() {
            Some(TimeMeasurement::begin("coverage::collection::post"))
        } else {
            None
        };

        let mut result = Vec::with_capacity(hooks.len());
        for (index, address) in hooks.iter().enumerate() {
            let address = address.align_even();

            let covered = self.interface.read_coverage_table(index);
            let last_rip = self.interface.read_last_rip(index);

            for (offset, (count, last_rip)) in
                covered.into_iter().zip(last_rip.into_iter()).enumerate()
            {
                result.push(if count > 0 {
                    ExecutionResultEntry::Covered {
                        address: address + offset,
                        count,
                        last_rip,
                    }
                } else {
                    ExecutionResultEntry::NotCovered {
                        address: address + offset,
                    }
                });
            }
        }

        #[cfg(feature = "timing_measurement")]
        drop(timing);

        result
    }

    pub fn setup(&mut self, hooks: &[UCInstructionAddress]) -> Result<(), CoverageError> {
        self.pre_execution(hooks)
    }

    pub fn is_hookable(&self, address: UCInstructionAddress) -> Result<(), NotHookableReason> {
        modification_engine::is_hookable(
            address,
            self.custom_processing_unit.rom(),
            &self.compile_mode,
        )
    }

    fn modify_triad_for_hooking(
        &self,
        hooked_address: UCInstructionAddress,
        mode: &ModificationEngineSettings,
    ) -> Result<[Triad; 2], NotHookableReason> {
        Ok([
            modification_engine::modify_triad_for_hooking(
                hooked_address.align_even(),
                self.custom_processing_unit.rom(),
                mode,
            )?,
            modification_engine::modify_triad_for_hooking(
                hooked_address.align_even() + 1,
                self.custom_processing_unit.rom(),
                mode,
            )?,
        ])
    }

    pub fn execute<FuncResult, F: FnOnce() -> FuncResult>(
        &mut self,
        hooks: &[UCInstructionAddress],
        func: F,
    ) -> Result<CoverageExecutionResult<FuncResult>, CoverageError> {
        #[cfg(feature = "timing_measurement")]
        let timing_measurement = if performance_timing::is_available() {
            Some(TimeMeasurement::begin("coverage::collection::execute"))
        } else {
            None
        };

        self.pre_execution(hooks)?;
        enable_hooks();

        lmfence();
        let result = core::hint::black_box(func)();
        lmfence();

        disable_all_hooks();
        let timing = self.post_execution(hooks);

        #[cfg(feature = "timing_measurement")]
        drop(timing_measurement);

        Ok(CoverageExecutionResult {
            result,
            hooks: timing,
        })
    }
}

#[cfg(feature = "ucode")]
impl Drop for CoverageHarness<'_, '_, '_> {
    fn drop(&mut self) {
        let _ = call_custom_ucode_function(coverage_collector::LABEL_FUNC_SETUP, [0, 0, 0]);

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
        count: CoverageCount,
        last_rip: u64,
    },
}

impl ExecutionResultEntry {
    pub fn address(&self) -> UCInstructionAddress {
        match self {
            ExecutionResultEntry::Covered { address, .. } => *address,
            ExecutionResultEntry::NotCovered { address } => *address,
        }
    }

    pub fn coverage(&self) -> u16 {
        match self {
            ExecutionResultEntry::Covered { count, .. } => *count,
            ExecutionResultEntry::NotCovered { .. } => 0,
        }
    }

    pub fn is_covered(&self) -> bool {
        self.coverage() > 0
    }
}

pub struct CoverageExecutionResult<R> {
    pub result: R,
    pub hooks: Vec<ExecutionResultEntry>,
}

impl<R: PartialEq> PartialEq for CoverageExecutionResult<R> {
    fn eq(&self, other: &Self) -> bool {
        self.result == other.result && self.hooks == other.hooks
    }
}

impl<R: Eq> Eq for CoverageExecutionResult<R> {}

impl<R: Clone> Clone for CoverageExecutionResult<R> {
    fn clone(&self) -> Self {
        CoverageExecutionResult {
            result: self.result.clone(),
            hooks: self.hooks.clone(),
        }
    }
}

impl<R: Debug> Debug for CoverageExecutionResult<R> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ExecutionResult")
            .field("result", &self.result)
            .field("hooks", &self.hooks)
            .finish()
    }
}
