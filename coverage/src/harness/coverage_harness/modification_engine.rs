//! This module provides functionality for modifying microcode triads to enable coverage collection.
//! It handles validation of hookable addresses and modification of instruction sequences while
//! maintaining proper execution semantics.

use data_types::addresses::UCInstructionAddress;
use ucode_compiler_dynamic::instruction::Instruction;
use ucode_compiler_dynamic::opcodes::Opcode;
use ucode_compiler_dynamic::sequence_word::{
    AssembleError, DisassembleError, SequenceWord, SequenceWordControl,
};
use ucode_compiler_dynamic::Triad;
use ucode_dump::RomDump;

bitflags::bitflags! {
    /// Settings for controlling microcode modifications
    ///
    /// These flags control what types of instructions and sequence words can be modified
    /// for coverage collection.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct ModificationEngineSettings: u64 {
        /// Disallow modification of GOTO instructions
        const NoGotos = 1 << 0;
        /// Disallow modification of SaveupIP sequence words
        const NoSaveupIPSequenceWords = 1 << 1;
        /// Disallow modification of SaveupIP register override instructions
        const NoSaveupIPRegOVRInstructions = 1 << 2;
        /// Disallow modification of SUBR instructions
        const NoSUBR = 1 << 3;
        /// Disallow modification of control operations
        const NoControl = 1 << 4;
        /// Disallow modification of sync operations
        const NoSync = 1 << 5;
        /// Disallow modification of conditional jumps
        const NoConditionalJumps = 1 << 6;
        /// Disallow modification of unknown instructions
        const NoUnknownInstructions = 1 << 7;
        /// Disallow modification of MOVEFROMCREG instructions
        const NoMoveFromCREG = 1 << 8;
    }
}

impl Default for ModificationEngineSettings {
    fn default() -> Self {
        Self::empty() | Self::NoUnknownInstructions
    }
}

/// Reasons why an address cannot be hooked
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NotHookableReason {
    /// The triad is already hooked
    TriadAlreadyHooked,
    /// The address is not in ROM
    AddressNotInRom,
    /// The address is not in the ROM dump
    AddressNotInDump,
    /// Failed to parse the sequence word
    ModificationFailedSequenceWordParse(DisassembleError),
    /// Failed to build the modified sequence word
    ModificationFailedSequenceWordBuild(AssembleError),
    /// Contains an unsupported control operation
    ControlOpPresent(SequenceWordControl),
    /// Contains an unsupported control operation (to be implemented)
    TodoControlOp,
    /// Contains an unsupported sync operation (to be implemented)
    TodoSyncOp,
    /// Contains an unsupported index (to be implemented)
    TodoIndexNotZero,
    /// Contains an unsupported jump (to be implemented)
    TodoJump,
    /// Contains a blacklisted instruction
    BlacklistedInstruction(Opcode),
    /// Address is blacklisted (to be implemented)
    TodoBlacklisted(UCInstructionAddress),
    /// Feature is disabled in settings
    FeatureDisabled(ModificationEngineSettings),
    /// Contains unsupported conditional moves (to be implemented)
    TodoNoConditionalMoves,
}

/// Checks if an address can be hooked for coverage collection
///
/// # Arguments
///
/// * `address` - The address to check
/// * `rom` - Reference to the ROM dump
/// * `mode` - Modification engine settings
///
/// # Returns
///
/// Ok(()) if the address can be hooked, or a NotHookableReason if it cannot
pub fn is_hookable(
    address: UCInstructionAddress,
    rom: &RomDump,
    mode: &ModificationEngineSettings,
) -> Result<(), NotHookableReason> {
    if address >= UCInstructionAddress::MSRAM_START {
        return Err(NotHookableReason::AddressNotInRom);
    }

    let address = address.align_even();

    let triad = Triad::try_from(
        rom.triad(address)
            .ok_or(NotHookableReason::AddressNotInDump)?,
    )
    .map_err(NotHookableReason::ModificationFailedSequenceWordParse)?;

    let instruction_pair = [
        triad.instructions[address.triad_offset() as usize + 0],
        if address.triad_offset() == 2 {
            Instruction::NOP
        } else {
            triad.instructions[address.triad_offset() as usize + 1]
        },
    ];
    let [_this_instruction, _next_instruction] = &instruction_pair;

    let sequence_word = match rom.get_sequence_word(address) {
        Some(sequence_word) => SequenceWord::disassemble_no_crc_check(sequence_word)
            .map_err(NotHookableReason::ModificationFailedSequenceWordParse)?,
        None => return Err(NotHookableReason::AddressNotInDump),
    };
    let sequence_word_view = sequence_word.view(address.align_even().triad_offset(), 2);

    if mode.contains(ModificationEngineSettings::NoSUBR)
        && instruction_pair
            .iter()
            .any(|instruction| instruction.opcode().is_group_SUBR())
    {
        return Err(NotHookableReason::FeatureDisabled(
            ModificationEngineSettings::NoSUBR,
        ));
    }

    if mode.contains(ModificationEngineSettings::NoGotos) && sequence_word_view.goto().is_some() {
        return Err(NotHookableReason::FeatureDisabled(
            ModificationEngineSettings::NoGotos,
        ));
    }

    if mode.contains(ModificationEngineSettings::NoSaveupIPSequenceWords)
        && sequence_word_view
            .control()
            .map(|c| c.value.is_saveupip())
            .unwrap_or(false)
    {
        return Err(NotHookableReason::FeatureDisabled(
            ModificationEngineSettings::NoSaveupIPSequenceWords,
        ));
    }

    if mode.contains(ModificationEngineSettings::NoConditionalJumps)
        && instruction_pair
            .iter()
            .any(|instruction| instruction.opcode().is_conditional_jump())
    {
        return Err(NotHookableReason::FeatureDisabled(
            ModificationEngineSettings::NoConditionalJumps,
        ));
    }

    if mode.contains(ModificationEngineSettings::NoSaveupIPRegOVRInstructions)
        && instruction_pair
            .iter()
            .any(|instruction| instruction.opcode() == Opcode::SAVEUIP_REGOVR)
    {
        return Err(NotHookableReason::FeatureDisabled(
            ModificationEngineSettings::NoSaveupIPRegOVRInstructions,
        ));
    }

    if mode.contains(ModificationEngineSettings::NoMoveFromCREG)
        && instruction_pair.iter().any(|instruction| {
            instruction.opcode().is_group_MOVEFROMCREG()
                || instruction.opcode().is_group_MOVETOCREG()
        })
    {
        return Err(NotHookableReason::FeatureDisabled(
            ModificationEngineSettings::NoMoveFromCREG,
        ));
    }

    if mode.contains(ModificationEngineSettings::NoUnknownInstructions)
        && triad
            .instructions
            .iter()
            .any(|instruction| instruction.opcode().is_group_UNKNOWN())
    {
        return Err(NotHookableReason::FeatureDisabled(
            ModificationEngineSettings::NoUnknownInstructions,
        ));
    }

    if mode.contains(ModificationEngineSettings::NoControl)
        && sequence_word_view.control().is_some()
    {
        return Err(NotHookableReason::FeatureDisabled(
            ModificationEngineSettings::NoControl,
        ));
    }

    if mode.contains(ModificationEngineSettings::NoSync) && sequence_word_view.sync().is_some() {
        return Err(NotHookableReason::FeatureDisabled(
            ModificationEngineSettings::NoSync,
        ));
    }

    if let Some(instruction) = triad
        .instructions
        .iter()
        .find(|instruction| instruction.opcode().is_group_TESTUSTATE())
    {
        return Err(NotHookableReason::BlacklistedInstruction(
            instruction.opcode(),
        ));
    }

    Ok(())
}

/// Modifies a triad to put it into the microcode after calling instrumentation
///
/// # Arguments
///
/// * `address` - The address to modify
/// * `rom` - Reference to the ROM dump
/// * `mode` - Modification engine settings
///
/// # Returns
///
/// The modified triad if successful, or a NotHookableReason if modification fails
pub fn modify_triad_for_hooking(
    address: UCInstructionAddress,
    rom: &RomDump,
    mode: &ModificationEngineSettings,
) -> Result<Triad, NotHookableReason> {
    is_hookable(address, rom, mode)?;

    let triad = Triad::try_from(
        rom.triad(address)
            .ok_or(NotHookableReason::AddressNotInDump)?,
    )
    .map_err(NotHookableReason::ModificationFailedSequenceWordParse)?;

    let instruction = *triad
        .instructions
        .get(address.triad_offset() as usize)
        .unwrap_or(&Instruction::NOP);

    let sequence_word = triad.sequence_word;

    let result_triad = Triad {
        instructions: [
            instruction,
            Instruction::NOP,
            Instruction::UJMP(address.next_address()),
        ],
        sequence_word: sequence_word
            .apply_view(address.triad_offset(), 1)
            .apply()
            .apply_shift(1),
    };

    if let Some(control) = result_triad.sequence_word.control() {
        if control.value.is_terminator() || control.value.is_saveupip() {
            // ok, we can handle this
        } else {
            // todo: other sequence word operations behavior unknown -> not hookable
            return Err(NotHookableReason::ControlOpPresent(control.value));
        }
    }

    let _ = result_triad
        .sequence_word
        .assemble()
        .map_err(NotHookableReason::ModificationFailedSequenceWordBuild)?;
    Ok(result_triad)
}

#[cfg(test)]
mod test {
    use super::*;
    use alloc::collections::BTreeMap;
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
        let settings = ModificationEngineSettings::default();

        let mut count = HashMap::new();
        let mut total = 0;
        for i in (0..0x7c00).map(UCInstructionAddress::from_const) {
            let result = is_hookable(i, &rom, &settings);
            total += 1;
            if let Err(err) = result {
                *count.entry(err).or_insert(0) += 1;
                continue;
            }
            if let Err(err) = modify_triad_for_hooking(i, &rom, &settings) {
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
            "{}",
            modify_triad_for_hooking(0x19d4.into(), &rom, &ModificationEngineSettings::default())
                .unwrap()
        );
    }

    #[test]
    fn print_address() {
        let rom = ucode_dump::dump::ROM_cpu_000506CA;
        let offset = 0x208c;
        for n in offset..offset + 4 {
            let address = UCInstructionAddress::from_const(n);
            print!("{}  ", address);
            let x = modify_triad_for_hooking(address, &rom, &ModificationEngineSettings::default())
                .unwrap();
            print!("{}", x);
            print!("  ");
            for i in x.instructions {
                print!("{:x?} ", i.assemble_no_crc());
            }
            println!();
        }
    }

    #[test]
    fn print_stats() {
        let total_addresses = 0x7c00;
        let micro_operations_count = 0x7c00 / 4 * 3;

        let mut hookable_yes = 0;
        let mut hookable_no = 0;
        let mut hookable_no_reasons = BTreeMap::new();

        let mut total_addresses_hookable = 0;

        for i in 0..total_addresses {
            let address = UCInstructionAddress::from_const(i);
            if address.triad_offset() != 0 {
                continue;
            }

            let low_hookable = modify_triad_for_hooking(
                address.triad_base(),
                &ucode_dump::dump::ROM_cpu_000506CA,
                &ModificationEngineSettings::default(),
            )
            .is_ok();
            let high_hookable = modify_triad_for_hooking(
                address.triad_base() + 2,
                &ucode_dump::dump::ROM_cpu_000506CA,
                &ModificationEngineSettings::default(),
            )
            .is_ok();

            hookable_yes += if low_hookable { 1 } else { 0 };
            hookable_no += if low_hookable { 0 } else { 1 };

            hookable_yes += if high_hookable { 1 } else { 0 };
            hookable_no += if high_hookable { 0 } else { 1 };

            if !low_hookable {
                let text = modify_triad_for_hooking(
                    address.triad_base(),
                    &ucode_dump::dump::ROM_cpu_000506CA,
                    &ModificationEngineSettings::default(),
                )
                .err()
                .unwrap();
                let text = format!("{:?}", text);
                *hookable_no_reasons.entry(text).or_insert(0) += 1;
            }
            if !high_hookable {
                let text = modify_triad_for_hooking(
                    address.triad_base() + 2,
                    &ucode_dump::dump::ROM_cpu_000506CA,
                    &ModificationEngineSettings::default(),
                )
                .err()
                .unwrap();
                let text = format!("{:?}", text);
                *hookable_no_reasons.entry(text).or_insert(0) += 1;
            }

            if low_hookable {
                total_addresses_hookable += 2;
            }
            if high_hookable {
                total_addresses_hookable += 1;
            }
        }

        println!("Total addresses: {}", total_addresses);
        println!("Micro operations count: {}", micro_operations_count);
        println!("Hookable YES: {}\t{}", hookable_yes, hookable_yes * 2);
        println!("Hookable NO: {}\t{}", hookable_no, hookable_no * 2);
        println!(
            "YES/NO: {}\t{}",
            hookable_yes + hookable_no,
            hookable_no * 2 + hookable_yes * 2
        );

        println!("Total addresses hookable: {}", total_addresses_hookable);

        println!("Reasons:");
        for (key, value) in hookable_no_reasons {
            println!(" - {}: {}", key, value);
        }
    }
}
