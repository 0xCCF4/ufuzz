use data_types::addresses::UCInstructionAddress;
use ucode_compiler::utils::instruction::Instruction;
use ucode_compiler::utils::opcodes::Opcode;
use ucode_compiler::utils::sequence_word::{
    AssembleError, DisassembleError, SequenceWord, SequenceWordControl,
};
use ucode_compiler::utils::Triad;
use ucode_dump::RomDump;

const RESTORE_CONTEXT: u64 = 0x6e75406aa00d; // LDSTGBUF_DSZ64_ASZ16_SC1(0xba40) !m2

bitflags::bitflags! {
    pub struct ModificationEngineSettings: u64 {
        const NoGotos = 1 << 0;
        const NoSaveupIPSequenceWords = 1 << 1;
        const NoSaveupIPRegOVRInstructions = 1 << 2;
        const NoSUBR = 1 << 3;
        const NoControl = 1 << 4;
        const NoSync = 1 << 5;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NotHookableReason {
    AddressNotInRom,
    AddressNotInDump,
    TriadAlreadyHooked,
    ModificationFailedSequenceWordParse(DisassembleError),
    ModificationFailedSequenceWordBuild(AssembleError),
    ControlOpPresent(SequenceWordControl),
    TodoControlOp,
    TodoSyncOp,
    TodoIndexNotZero,
    TodoJump,
    LBSYNC,
    TodoBlacklisted(UCInstructionAddress),
    TodoConditionalJump,
    TESTUSTATE,
    FeatureDisabled,
}

pub fn is_hookable(address: UCInstructionAddress, rom: &RomDump, mode: &ModificationEngineSettings) -> Result<(), NotHookableReason> {
    if address >= UCInstructionAddress::MSRAM_START {
        return Err(NotHookableReason::AddressNotInRom);
    }

    let address = address.align_even();

    let instruction_pair = rom
        .get_instruction_pair(address)
        .ok_or(NotHookableReason::AddressNotInRom)?;
    let instruction_pair = [
        Instruction::disassemble(instruction_pair[0]),
        Instruction::disassemble(instruction_pair[1]),
    ];
    let [_this_instruction, _next_instruction] = &instruction_pair;

    let sequence_word = match rom.get_sequence_word(address) {
        Some(sequence_word) => SequenceWord::disassemble_no_crc_check(sequence_word as u32)
            .map_err(NotHookableReason::ModificationFailedSequenceWordParse)?,
        None => return Err(NotHookableReason::AddressNotInDump),
    };
    let sequence_word_view = sequence_word.view(address.align_even().triad_offset(), 2);

    if mode.contains(ModificationEngineSettings::NoSUBR) {
        if instruction_pair
            .iter()
            .any(|instruction| instruction.opcode().is_group_TESTUSTATE()) {
            return Err(NotHookableReason::FeatureDisabled);
        }
    }

    if mode.contains(ModificationEngineSettings::NoGotos) {
        if sequence_word_view.goto().is_some() {
            return Err(NotHookableReason::FeatureDisabled);
        }
    }

    if mode.contains(ModificationEngineSettings::NoSaveupIPSequenceWords) {
        if sequence_word_view.control().map(|c| c.value.is_saveupip()).unwrap_or(false) {
            return Err(NotHookableReason::FeatureDisabled);
        }
    }

    if mode.contains(ModificationEngineSettings::NoSaveupIPRegOVRInstructions) {
        if instruction_pair
            .iter()
            .any(|instruction| instruction.opcode() == Opcode::SAVEUIP_REGOVR)
        {
            return Err(NotHookableReason::FeatureDisabled);
        }
    }

    if mode.contains(ModificationEngineSettings::NoControl) {
        if sequence_word_view.control().is_some() {
            return Err(NotHookableReason::FeatureDisabled);
        }
    }

    if mode.contains(ModificationEngineSettings::NoControl) {
        if sequence_word_view.sync().is_some() {
            return Err(NotHookableReason::FeatureDisabled);
        }
    }

    if instruction_pair
        .iter()
        .any(|instruction| instruction.opcode().is_group_TESTUSTATE())
    {
        return Err(NotHookableReason::TESTUSTATE);
    }

    if instruction_pair
        .iter()
        .any(|instruction| instruction.opcode() == Opcode::LBSYNC)
    {
        return Err(NotHookableReason::LBSYNC);
    }

    let blacklist: &[usize] = &[
        // is this lfence instruction?
        0xd0,  // U00d0: 000000000000 NOP
        0xd1,  // U00d1: 000000000000 LFNCEMARK-> NOP SEQW GOTO U008e
        0x8e,  // LFNCEWAIT-> NOP SEQW UEND0
        0x3c8, // U03c8: 004100030001 tmp0:= OR_DSZ64(r64dst)
        0x3c9, // U03c9: 100800001042 r64dst:= ZEROEXT_DSZ32N(r64src, r64dst) !m1
        0x3ca, // U03ca: 1008000020b0 r64src:= ZEROEXT_DSZ32N(tmp0, r64src) !m1 SEQW UEND0
    ];

    if blacklist
        .iter()
        .any(|a| UCInstructionAddress::from_const(*a).align_even() == address.align_even())
        && rom.model() == 0x506CA
    {
        return Err(NotHookableReason::TodoBlacklisted(address));
    }

    Ok(())
}

pub fn modify_triad_for_hooking(
    address: UCInstructionAddress,
    rom: &RomDump,
    mode: &ModificationEngineSettings,
) -> Result<[Triad; 2], NotHookableReason> {
    is_hookable(address, rom, mode)?;

    let triad = rom
        .triad(address)
        .ok_or(NotHookableReason::AddressNotInDump)?;

    let instruction = Instruction::disassemble(
        *triad
            .instructions
            .get(address.triad_offset() as usize)
            .unwrap_or(&0u64),
    );

    let sequence_word = SequenceWord::disassemble_no_crc_check(triad.sequence_word as u32)
        .map_err(NotHookableReason::ModificationFailedSequenceWordParse)?;

    let mut result_triad = Triad {
        instructions: [
            Instruction::disassemble(RESTORE_CONTEXT),
            instruction,
            Instruction::NOP,
        ],
        sequence_word: sequence_word
            .apply_view(address.triad_offset(), 1)
            .apply()
            .apply_shift(1),
    };

    let next_triad = Triad {
        instructions: [Instruction::NOP, Instruction::NOP, Instruction::NOP],
        sequence_word: SequenceWord::new().apply_goto(0, address.next_address()), // when using SEQW SAVEUIP or SAVEUIP_REGOVR() this will restore the control flow
    };

    if let Some(control) = result_triad.sequence_word.control() {
        if control.value.is_terminator() || control.value.is_saveupip() {
            // ok, we can handle this
        } else {
            // todo: other sequence word operations behavior unknown -> not hookable
            return Err(NotHookableReason::ControlOpPresent(control.value));
        }
    }

    match result_triad.sequence_word.goto() {
        Some(_goto) => {
            // do nothing, since jump will be automatically taken
        }
        None => {
            let terminator = if let Some(control) = result_triad.sequence_word.control() {
                if control.value.is_terminator() {
                    true
                } else {
                    false
                }
            } else {
                false
            };

            if !terminator {
                result_triad
                    .sequence_word
                    .set_goto(1, address.next_address());
            }
        }
    }

    let _ = result_triad
        .sequence_word
        .assemble()
        .map_err(NotHookableReason::ModificationFailedSequenceWordBuild)?;
    let _ = next_triad
        .sequence_word
        .assemble()
        .map_err(NotHookableReason::ModificationFailedSequenceWordBuild)?;
    Ok([result_triad, next_triad])
}

#[cfg(test)]
mod test {
    use super::*;
    use std::collections::HashMap;
    use std::println;
    use std::string::ToString;

    #[test]
    fn test_if_multiple_saveup_per_triad_pair() {
        let rom = ucode_dump::dump::ROM_cpu_000506CA;

        for address in (0..0x7c00)
            .filter(|p| p % 2 == 0)
            .map(UCInstructionAddress::from_const)
        {
            let pair = rom
                .get_instruction_pair(address)
                .unwrap()
                .map(Instruction::disassemble);
            let sequence_word =
                SequenceWord::disassemble_no_crc_check(rom.get_sequence_word(address).unwrap())
                    .unwrap()
                    .apply_view(address.triad_offset(), 2)
                    .apply();

            let pair_save = pair
                .iter()
                .filter(|instruction| instruction.opcode() == Opcode::SAVEUIP_REGOVR)
                .count();
            let sequence_word_save =
                if let Some(control) = sequence_word.view(address.triad_offset(), 2).control() {
                    control.value.is_saveupip() as usize
                } else {
                    0
                };

            if pair_save + sequence_word_save > 1 {
                print!("{}: {:?} ", address, pair.map(|o| o.opcode()));
                println!("{}", sequence_word.to_string());
            }
        }
    }

    #[test]
    fn test_offline_modification() {
        let rom = ucode_dump::dump::ROM_cpu_000506CA;
        let mut count = HashMap::new();
        let mut total = 0;
        for i in (0..0x7c00).map(UCInstructionAddress::from_const) {
            let result = is_hookable(i, &rom);
            total += 1;
            if let Err(err) = result {
                *count.entry(err).or_insert(0) += 1;
                continue;
            }
            if let Err(err) = modify_triad_for_hooking(i, &rom) {
                *count.entry(err).or_insert(0) += 1;
                continue;
            }
        }

        println!("Total: {}", total);
        let skipped = count.values().sum::<usize>();
        println!(
            "Skipped: {} {}%",
            skipped,
            skipped as f64 / total as f64 * 100.0
        );
        println!(
            "Worked: {} {}%",
            total - skipped,
            (total - skipped) as f64 / total as f64 * 100.0
        );

        println!("Reasons:");

        for (key, value) in count {
            println!(" - {:?}: {}", key, value);
        }

        println!("{}", modify_triad_for_hooking(0x19d4.into(), &rom).unwrap().map(|x| format!("{}", x)).join(" ; "));
    }
}
