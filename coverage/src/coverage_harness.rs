use crate::coverage_collector;
use crate::interface::safe::ComInterface;
use crate::interface_definition::{CoverageEntry, InstructionTableEntry};
#[cfg(feature = "no_std")]
use alloc::vec::Vec;
use custom_processing_unit::{apply_patch, call_custom_ucode_function, disable_all_hooks, enable_hooks, lmfence, restore_hooks, CustomProcessingUnit, FunctionResult};
use data_types::addresses::{Address, UCInstructionAddress};
use itertools::Itertools;
use ucode_compiler::utils::instruction::Instruction;
use ucode_compiler::utils::sequence_word::{DisassembleError, SequenceWord};

const COVERAGE_ENTRIES: usize = UCInstructionAddress::MAX.to_const();

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoverageError {
    TooManyHooks,
    SetupFailed(FunctionResult),
    AddressNotHookable(UCInstructionAddress, NotHookableReason),
    SequenceWordDissembleError(DisassembleError),
}

pub struct CoverageHarness<'a, 'b, 'c> {
    interface: &'a mut ComInterface<'b>,
    coverage: [CoverageEntry; COVERAGE_ENTRIES], // every forth entry, beginning at 3 is zero
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

        let mut instructions = Vec::with_capacity(hooks.len());
        for hook in hooks {
            if let Err(err) = self.is_hookable(*hook) {
                return Err(CoverageError::AddressNotHookable(*hook, err));
            }

            let entry: InstructionTableEntry = [
                self.custom_processing_unit
                    .rom()
                    .get_instruction(hook.triad_base() + 0)
                    .expect("Since is_hookable is true. Address is bound to be in ROM"),
                self.custom_processing_unit
                    .rom()
                    .get_instruction(hook.triad_base() + 1)
                    .expect("Since is_hookable is true. Address is bound to be in ROM"),
                self.custom_processing_unit
                    .rom()
                    .get_instruction(hook.triad_base() + 2)
                    .expect("Since is_hookable is true. Address is bound to be in ROM"),
                self.custom_processing_unit
                    .rom()
                    .get_sequence_word(hook.triad_base())
                    .expect("Since is_hookable is true. Address is bound to be in ROM")
                    as u64,
            ];

            instructions.push(
                self.modify_triad_for_hooking(*hook, entry)
                    .map_err(|err| CoverageError::AddressNotHookable(*hook, err))?,
            );
        }

        self.interface.write_jump_table_all(hooks);
        self.interface.write_instruction_table_all(instructions);

        let result =
            call_custom_ucode_function(coverage_collector::LABEL_FUNC_SETUP, [hooks.len(), 0, 0]);

        if result.rax != 0x664200006642 {
            return Err(CoverageError::SetupFailed(result));
        }

        Ok(())
    }

    #[inline(always)]
    fn post_execution(&mut self, hooks: &[UCInstructionAddress]) {
        for (index, address) in hooks.iter().enumerate() {
            let covered = self.interface.read_coverage_table(index);
            self.coverage[address.address()] += covered;
        }
    }

    pub fn setup(&mut self, hooks: &[UCInstructionAddress]) -> Result<(), CoverageError> {
        self.pre_execution(hooks)
    }

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

    pub fn get_coverage(&self) -> &[CoverageEntry; COVERAGE_ENTRIES] {
        &self.coverage
    }

    pub fn is_hookable(&self, address: UCInstructionAddress) -> Result<(), NotHookableReason> {
        if address >= UCInstructionAddress::MSRAM_START {
            return Err(NotHookableReason::AddressNotInRom);
        }

        if address.address() % 4 != 0 && address.address() % 4 != 2 {
            return Err(NotHookableReason::AddressNotAligned);
        }

        let instruction = match self.custom_processing_unit.rom().get_instruction(address) {
            Some(instruction) => Instruction::from(instruction),
            None => return Err(NotHookableReason::AddressNotInDump),
        };

        let opcode = instruction.opcode();

        if opcode.is_group_TESTUSTATE() || opcode.is_group_SUBR() {
            return Err(NotHookableReason::ConditionalJump);
        }

        if opcode.is_conditional_jump() {
            return Err(NotHookableReason::TodoCondJump);
        }

        Ok(())
    }

    fn modify_triad_for_hooking(
        &self,
        hooked_address: UCInstructionAddress,
        triad: InstructionTableEntry,
    ) -> Result<InstructionTableEntry, NotHookableReason> {
        self.is_hookable(hooked_address)?;

        if hooked_address.triad_offset() != 0 {
            return Err(NotHookableReason::TodoIndexNotZero);
        }

        let mut sequence_word = SequenceWord::disassemble_no_crc_check(triad[3] as u32)
            .map_err(NotHookableReason::ModificationFailedSequenceWord)?;
        let instructions = triad
            .into_iter()
            .map(Instruction::disassemble)
            .map(|i| i.assemble())
            .collect_vec();

        if sequence_word.control().is_some() {
            return Err(NotHookableReason::ControlOpPresent);
        }
        if sequence_word.sync().is_some() {
            return Err(NotHookableReason::TodoSyncOp);
        }

        match sequence_word.goto().clone() {
            None => {
                sequence_word.set_goto(hooked_address.triad_offset(), hooked_address.next_address());
            }
            Some(goto) => {
                if let Some(control) = sequence_word.control() {
                    if control.apply_to_index == hooked_address.triad_offset() {
                        // both control and goto not allowed at same offset, see uasm.py
                        return Err(NotHookableReason::ControlOpPresent); // TODO
                    }
                    return Err(NotHookableReason::TodoControlOp); // TODO
                }
                if sequence_word.sync().is_some() {
                    return Err(NotHookableReason::TodoSyncOp); // TODO
                }

                //return Ok(None); // TODO


                if goto.apply_to_index == hooked_address.triad_offset() {
                    // since we are jumping anyway -> no modification needed
                } else {
                    // jump is later or previously in triad -> modify it
                    sequence_word.set_goto(hooked_address.triad_offset(), hooked_address.next_address());
                }
            }
        }

        sequence_word.no_goto().no_sync().no_control().set_goto(1, 0x42a);

        Ok([
            instructions[0],
            instructions[1],
            instructions[2],
            sequence_word.assemble() as u64,
        ])
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NotHookableReason {
    AddressNotAligned,
    AddressNotInRom,
    AddressNotInDump,
    ConditionalJump,
    ModificationFailedSequenceWord(DisassembleError),
    ControlOpPresent,
    TodoControlOp,
    TodoSyncOp,
    TodoIndexNotZero,
    TodoCondJump,
}
