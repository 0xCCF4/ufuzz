use itertools::Itertools;
use data_types::addresses::{Address, UCInstructionAddress};
use data_types::patch::Triad;
use ucode_compiler::utils::instruction::Instruction;
use ucode_compiler::utils::opcodes::Opcode;
use ucode_compiler::utils::sequence_word::{AssembleError, DisassembleError, SequenceWord, SequenceWordControl};
use ucode_dump::RomDump;


#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NotHookableReason {
    AddressNotAligned,
    AddressNotInRom,
    AddressNotInDump,
    ModificationFailedSequenceWordParse(DisassembleError),
    ModificationFailedSequenceWordBuild(AssembleError),
    ControlOpPresent(SequenceWordControl),
    TodoControlOp,
    TodoSyncOp,
    TodoIndexNotZero,
    TodoCondJump,
    TodoJump,
    TodoConditionalJump, // needs second triad to jump back
    TodoSaveUIP, // needs second triad to jump back
    LBSYNC,
    TodoBlacklisted(UCInstructionAddress),
    OddJumpTowardsHookPair,
}

pub fn is_hookable(address: UCInstructionAddress, rom: &RomDump) -> Result<(), NotHookableReason> {
    if address >= UCInstructionAddress::MSRAM_START {
        return Err(NotHookableReason::AddressNotInRom);
    }

    if address.address() % 4 != 0 && address.address() % 4 != 2 {
        return Err(NotHookableReason::AddressNotAligned);
    }

    let blacklist = include!("../blacklist.gen");
    if blacklist.contains(&address.address()) {
        return Err(NotHookableReason::OddJumpTowardsHookPair);
    }

    let hooked_instruction = match rom.get_instruction(address) {
        Some(instruction) => Instruction::from(instruction),
        None => return Err(NotHookableReason::AddressNotInDump),
    };
    let next_instruction = match rom.get_instruction(address+1) {
        Some(instruction) => Instruction::from(instruction),
        None => return Err(NotHookableReason::AddressNotInDump),
    };

    let sequence_word = match rom.get_sequence_word(address) {
        Some(sequence_word) => SequenceWord::disassemble_no_crc_check(sequence_word as u32)
            .map_err(NotHookableReason::ModificationFailedSequenceWordParse)?,
        None => return Err(NotHookableReason::AddressNotInDump),
    };

    let opcodes = [hooked_instruction.opcode(), next_instruction.opcode()];

    if hooked_instruction.opcode() == Opcode::SAVEUIP_REGOVR {
        return Err(NotHookableReason::TodoSaveUIP);
    }

    if next_instruction.opcode() == Opcode::SAVEUIP_REGOVR {
        if let Some(control) = sequence_word.control() {
            if control.value.is_terminator() && control.apply_to_index == address.triad_offset() {
                // this is fine, since we will return anyway
            } else {
                return Err(NotHookableReason::TodoSaveUIP);
            }
        } else {
            return Err(NotHookableReason::TodoSaveUIP);
        }
    }

    if let Some(control) = sequence_word.control() {
        if (control.apply_to_index == address.triad_offset() || control.apply_to_index == address.triad_offset()+1) &&
            control.value.is_saveupip() {
            return Err(NotHookableReason::TodoSaveUIP);
        }
    }

    if opcodes.iter().any(|opcode | opcode.is_group_TESTUSTATE() || opcode.is_group_SUBR()) {
        return Err(NotHookableReason::TodoConditionalJump);
    }

    if opcodes.iter().any(|opcode| *opcode == Opcode::LBSYNC) {
        return Err(NotHookableReason::LBSYNC);
    }

    if address == 0x8eusize && rom.model() == 0x506CA {
        // LFNCEWAIT-> NOP SEQW UEND0
        // TODO: is this an exception or is uend0 not hookable?
        return Err(NotHookableReason::TodoBlacklisted(address));
    }
    if (address >= 0x3c8usize && address <= 0x3causize) && rom.model() == 0x506CA {
        // U03c8: 004100030001 tmp0:= OR_DSZ64(r64dst)
        // U03c9: 100800001042 r64dst:= ZEROEXT_DSZ32N(r64src, r64dst) !m1
        // U03ca: 1008000020b0 r64src:= ZEROEXT_DSZ32N(tmp0, r64src) !m1 SEQW UEND0

        // this seems to be a swapping microcode. No other microcode jumping to that addresses.

        // TODO: is this an exception or is uend0 not hookable?
        return Err(NotHookableReason::TodoBlacklisted(address));
    }

    Ok(())
}

pub fn modify_triad_for_hooking(
    hooked_address: UCInstructionAddress,
    rom: &RomDump,
) -> Result<Triad, NotHookableReason> {
    is_hookable(hooked_address, rom)?;

    let triad = rom.triad(hooked_address).ok_or(NotHookableReason::AddressNotInDump)?;

    let mut sequence_word = SequenceWord::disassemble_no_crc_check(triad.sequence_word as u32)
        .map_err(NotHookableReason::ModificationFailedSequenceWordParse)?;
    let instructions = triad
        .instructions
        .into_iter()
        .map(Instruction::disassemble)
        .map(|i| i.assemble())
        .collect_vec();

    let jump_idx = (hooked_address.triad_offset()+1).min(2);

    if let Some(control) = sequence_word.control() {
        if jump_idx == control.apply_to_index {
            if control.value.is_terminator() {
                // do nothing, since we will return anyway
                return Ok(Triad {
                    instructions: [
                        instructions[0],
                        instructions[1],
                        instructions[2],
                    ],
                    sequence_word: sequence_word.assemble().map_err(NotHookableReason::ModificationFailedSequenceWordBuild)?,
                });
            } else {
                return Err(NotHookableReason::ControlOpPresent(control.value));
            }
        }
    }

    match sequence_word.goto() {
        Some(goto) => {
            if goto.apply_to_index == jump_idx || goto.apply_to_index == hooked_address.triad_offset() {
                // do nothing, since jump will be automatically taken
            } else {
                sequence_word.set_goto(jump_idx, hooked_address.next_even_address());
            }
        }
        None => {
            sequence_word.set_goto(jump_idx, hooked_address.next_even_address());
        }
    }

    Ok(Triad {
        instructions: [
            instructions[0],
            instructions[1],
            instructions[2],
        ],
        sequence_word: sequence_word.assemble().map_err(NotHookableReason::ModificationFailedSequenceWordBuild)?,
    })
}


#[cfg(test)]
mod test {
    use std::collections::HashMap;
    use std::println;
    use super::*;

    #[test]
    fn test_offline_modification() {
        let rom = ucode_dump::dump::ROM_cpu_000506CA;
        let mut count = HashMap::new();
        let mut total = 0;
        for i in (0..0x7c00).filter(|v| matches!(v % 4, 0 | 2)).map(UCInstructionAddress::from_const) {
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
        println!("Skipped: {} {}%", skipped, skipped as f64 / total as f64 * 100.0);
        println!("Worked: {} {}%", total - skipped, (total - skipped) as f64 / total as f64 * 100.0);

        println!("Reasons:");

        for (key, value) in count {
            println!(" - {:?}: {}", key, value);
        }

        println!("{:x?}", modify_triad_for_hooking(0x19d4.into(), &rom));
    }
}