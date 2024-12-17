use data_types::addresses::UCInstructionAddress;
use ucode_compiler::utils::instruction::Instruction;
use ucode_compiler::utils::opcodes::Opcode;
use ucode_compiler::utils::sequence_word::{
    AssembleError, DisassembleError, SequenceWord, SequenceWordControl,
};
use ucode_compiler::utils::Triad;
use ucode_dump::RomDump;

const RESTORE_CONTEXT: u64 = 0x6e75406aa00d; // LDSTGBUF_DSZ64_ASZ16_SC1(0xba40) !m2

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
    TodoCondJump,
    TodoJump,
    TodoConditionalJump, // needs second triad to jump back
    TodoSaveUIP,         // needs second triad to jump back
    LBSYNC,
    TodoBlacklisted(UCInstructionAddress),
}

pub fn is_hookable(address: UCInstructionAddress, rom: &RomDump) -> Result<(), NotHookableReason> {
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
    let [this_instruction, next_instruction] = &instruction_pair;

    let sequence_word = match rom.get_sequence_word(address) {
        Some(sequence_word) => SequenceWord::disassemble_no_crc_check(sequence_word as u32)
            .map_err(NotHookableReason::ModificationFailedSequenceWordParse)?,
        None => return Err(NotHookableReason::AddressNotInDump),
    };
    let sequence_word_view = sequence_word.view(address.align_even().triad_offset(), 2);

    if this_instruction.opcode() == Opcode::SAVEUIP_REGOVR {
        return Err(NotHookableReason::TodoSaveUIP);
    }

    if next_instruction.opcode() == Opcode::SAVEUIP_REGOVR {
        if let Some(control) = sequence_word_view.control() {
            if control.value.is_terminator() && control.apply_to_index == 1 {
                // this is fine, since we will return anyway
            } else {
                return Err(NotHookableReason::TodoSaveUIP);
            }
        } else {
            return Err(NotHookableReason::TodoSaveUIP);
        }
    }

    if let Some(control) = sequence_word_view.control() {
        if control.value.is_saveupip() {
            return Err(NotHookableReason::TodoSaveUIP);
        }
    }

    if instruction_pair.iter().any(|instruction| {
        instruction.opcode().is_group_TESTUSTATE() || instruction.opcode().is_group_SUBR()
    }) {
        return Err(NotHookableReason::TodoConditionalJump);
    }

    if instruction_pair
        .iter()
        .any(|instruction| instruction.opcode() == Opcode::LBSYNC)
    {
        return Err(NotHookableReason::LBSYNC);
    }

    let blacklist: &[usize] = &[
        // is this lfence instruction?
        0xd0, // U00d0: 000000000000 NOP
        0xd1, // U00d1: 000000000000 LFNCEMARK-> NOP SEQW GOTO U008e
        0x8e, // LFNCEWAIT-> NOP SEQW UEND0
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
) -> Result<[Triad; 2], NotHookableReason> {
    is_hookable(address, rom)?;

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
        sequence_word: SequenceWord::new()
            .apply_goto(0, crate::coverage_collector::LABEL_EXIT_TRAP),
    };

    if let Some(control) = result_triad.sequence_word.control() {
        if control.value.is_terminator() {
            // do nothing, since we will return anyway

            let _ = result_triad
                .sequence_word
                .assemble()
                .map_err(NotHookableReason::ModificationFailedSequenceWordBuild)?;
            let _ = next_triad
                .sequence_word
                .assemble()
                .map_err(NotHookableReason::ModificationFailedSequenceWordBuild)?;
            return Ok([result_triad, next_triad]);
        } else {
            return Err(NotHookableReason::ControlOpPresent(control.value));
        }
    }

    match result_triad.sequence_word.goto() {
        Some(_goto) => {
            // do nothing, since jump will be automatically taken
        }
        None => {
            result_triad
                .sequence_word
                .set_goto(1, address.next_address());
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

        println!(
            "{:?}",
            SequenceWord::disassemble(
                modify_triad_for_hooking(0xd0.into(), &rom)
                    .unwrap()
                    .sequence_word
            )
        );
        println!(
            "{:?}",
            SequenceWord::disassemble(
                modify_triad_for_hooking(0xd1.into(), &rom)
                    .unwrap()
                    .sequence_word
            )
        );
    }
}
