use data_types::addresses::UCInstructionAddress;
use ucode_compiler_dynamic::instruction::Instruction;
use ucode_compiler_dynamic::opcodes::Opcode;
use ucode_compiler_dynamic::sequence_word::{
    AssembleError, DisassembleError, SequenceWord, SequenceWordControl,
};
use ucode_compiler_dynamic::Triad;
use ucode_dump::RomDump;

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct ModificationEngineSettings: u64 {
        const NoGotos = 1 << 0;
        const NoSaveupIPSequenceWords = 1 << 1;
        const NoSaveupIPRegOVRInstructions = 1 << 2;
        const NoSUBR = 1 << 3;
        const NoControl = 1 << 4;
        const NoSync = 1 << 5;
        const NoConditionalJumps = 1 << 6;
        const NoUnknownInstructions = 1 << 7;
        const NoMoveFromCREG = 1 << 8;
    }
}

impl Default for ModificationEngineSettings {
    fn default() -> Self {
        Self::empty() | Self::NoUnknownInstructions
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NotHookableReason {
    TriadAlreadyHooked,
    AddressNotInRom,
    AddressNotInDump,
    ModificationFailedSequenceWordParse(DisassembleError),
    ModificationFailedSequenceWordBuild(AssembleError),
    ControlOpPresent(SequenceWordControl),
    TodoControlOp,
    TodoSyncOp,
    TodoIndexNotZero,
    TodoJump,
    BlacklistedInstruction(Opcode),
    TodoBlacklisted(UCInstructionAddress),
    FeatureDisabled(ModificationEngineSettings),
    TodoNoConditionalMoves,
}

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

    if let Some(instruction) = instruction_pair
        .iter()
        // todo check if all LBSYNCs are bad
        // when hooking unk118/222, CPU freezes, since behavior of unk118 not known, just accept this -> blacklist
        .find(|instruction| {
            matches!(
                instruction.opcode(), // todo check 118/222
                Opcode::LBSYNC | Opcode::UNKNOWN_118 | Opcode::UNKNOWN_222
            )
        })
    {
        return Err(NotHookableReason::BlacklistedInstruction(
            instruction.opcode(),
        ));
    }

    let blacklist: &[usize] = &[
        // when hooking one of those instructions, CPU freezes, unknown reasons, maybe investigate more?
        // instructions seem very simple, cant come up with a reason why hooking does not work
        // maybe some reorder/scheduling magic, when updating hooks?

        /* // is this lfence instruction?
        0xd0,  // U00d0: 000000000000 NOP
        0xd1,  // U00d1: 000000000000 LFNCEMARK-> NOP SEQW GOTO U008e
        0x8e,  // LFNCEWAIT-> NOP SEQW UEND0
        0x3c8, // U03c8: 004100030001 tmp0:= OR_DSZ64(r64dst)
        0x3c9, // U03c9: 100800001042 r64dst:= ZEROEXT_DSZ32N(r64src, r64dst) !m1
        0x3ca, // U03ca: 1008000020b0 r64src:= ZEROEXT_DSZ32N(tmp0, r64src) !m1 SEQW UEND0
        0x3f0, // U03f0: 3c1a00e30144 tmp0:= LDTICKLE_DSZ32_ASZ32_SC1(r64base, DS, r64idx, IMM_MACRO_ALIAS_DISPLACEMENT, mode=0x18) !m0,m1,m2
        0x3f1, // U03f1: 3c1800e01144 STAD_DSZ32_ASZ32_SC1(r64base, DS, r64idx, IMM_MACRO_ALIAS_DISPLACEMENT, mode=0x18, r64dst) !m0,m1,m2
        0x3f2, // U03f2: 100800001070 r64dst:= ZEROEXT_DSZ32N(tmp0, r64dst) !m1 SEQW UEND0
        0x434, 0x442, 0x446, 0x44a, 0x452, 0x45c, 0x458, 0x45a, 0x45e, 0x460, 0x462, 0x464, 0x466,
        0x468, 0x46e, 0x46c, 0x46a,
        0x470, // U0470: 0c9a00e33144 tmp3:= LDTICKLE_DSZ16_ASZ32_SC1(r64base, DS, r64idx, IMM_MACRO_ALIAS_DISPLACEMENT, mode=0x18) !m0
        0x471, // U0471: 00c800830008 tmp0:= ZEROEXT_DSZ8(IMM_MACRO_ALIAS_IMMEDIATE) !m0
        0x472, // U0472: 000c7c940200 SAVEUIP( , 0x01, U057c) !m0 SEQW GOTO U0982
        0x474, // U0474: 0001c8032c90 tmp2:= OR_DSZ32(0x00100000, tmp2)
        0x475, // U0475: 000100032cb1 tmp2:= OR_DSZ32(tmp1, tmp2)
        0x476,
        0x478, // U0478: 0c9a00e33144 tmp3:= LDTICKLE_DSZ16_ASZ32_SC1(r64base, DS, r64idx, IMM_MACRO_ALIAS_DISPLACEMENT, mode=0x18) !m0
        0x479, // U0479: 004100030021 tmp0:= OR_DSZ64(rcx)
        0x47a, // U047a: 000c7c940200 SAVEUIP( , 0x01, U057c) !m0 SEQW GOTO U0982
        // U047c: 200a24800200 TESTUSTATE( , VMX, !0x0024) !m0,m2 ? SEQW GOTO U57cd
        // U047d: 0062bb1f3200 tmp3:= MOVEFROMCREG_DSZ64( , 0x7bb)
        0x47e, // U047e: 186b119c02b3 BTUJNB_DIRECT_NOTTAKEN(tmp3, 0x0000000a, U2711) !m0,m1 SEQW URET1
        0x480, // U0480: 0cd000e30144 tmp0:= LDZX_DSZ8_ASZ32_SC1(r64base, DS, r64idx, IMM_MACRO_ALIAS_DISPLACEMENT, mode=0x18) !m0
        0x481, // U0481: 00ef00030030 tmp0:= unk_0ef(tmp0)
        0x482, // U0482: 008800001070 r64dst:= ZEROEXT_DSZ16(tmp0, r64dst) SEQW UEND0
        // todo investigate this, later instructions fails first
        0x484, // U0484: 002406031231 tmp1:= SHL_DSZ32(tmp1, 0x00000006)
        0x485, // U0485: 000704331231 tmp1:= NOTAND_DSZ32(tmp1, 0x00000c04)
        0x486, // U0486: 004000035d71 tmp5:= ADD_DSZ64(tmp1, tmp5) SEQW GOTO U189a
        // todo investigate this, later instructions fails first
        0x488, // U0488: 2062fe1f5200 tmp5:= MOVEFROMCREG_DSZ64( , 0x7fe) !m2
        0x489, // U0489: 000100135d48 tmp5:= OR_DSZ32(0x00000400, tmp5)
        0x490,
        0x48a, // U048a: 2a62fe1c0335 SYNCFULL-> MOVETOCREG_BTR_DSZ64(tmp5, 0x00000010, 0x7fe) !m2 SEQW GOTO U221e
        // todo investigate this
        0x48c, // already blacklisted unk222
        0x48d, // already blacklisted unk222
        0x48e, // U048e: 0064ff7fdf5f tmp13:= SHL_DSZ64(0xffffffffffffffff, tmp13) SEQW GOTO U078d
        0x68e, // todo generalize?
        0x9b6, 0x9dc, 0x9de, 0x3b58, 0x48b0, 0x63d8, 0x6452, */
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
            Instruction::UJMP(address.next_address()),
            Instruction::NOP,
        ],
        sequence_word: sequence_word.apply_view(address.triad_offset(), 1).apply(),
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
            modify_triad_for_hooking(0x19d4.into(), &rom)
                .unwrap()
                .map(|x| format!("{}", x))
                .join(" ; ")
        );
    }
}
