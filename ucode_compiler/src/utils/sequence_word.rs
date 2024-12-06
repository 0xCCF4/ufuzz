use data_types::addresses::{Address, UCInstructionAddress};
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;
use crate::utils::even_odd_parity_u32;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, FromPrimitive)]
#[allow(non_camel_case_types)] // these are the names
pub enum SequenceWordControl {
    URET0 = 0x2,
    URET1 = 0x3,

    SAVEUPIP0 = 0x4,
    SAVEUPIP1 = 0x5,

    ROVR_SAVEUPIP0 = 0x6,
    ROVR_SAVEUPIP1 = 0x7,

    WRTAGW = 0x8,
    MSLOOP = 0x9,
    MSSTOP = 0xB,

    UEND0 = 0xC,
    UEND1 = 0xD,
    UEND2 = 0xE,
    UEND3 = 0xF,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, FromPrimitive)]
pub enum SequenceWordSync {
    LFNCEWAIT = 0x1,
    LFNCEMARK = 0x2,
    LFNCEWTMRK = 0x3,
    SYNCFULL = 0x4,
    SYNCWAIT = 0x5,
    SYNCMARK = 0x6,
    SYNCWTMRK = 0x7,
}

#[derive(Clone, Debug)]
pub struct SequenceWordPart<T> {
    pub apply_to_index: u8,
    pub value: T,
}

/// A sequence word is a modifier applied to a triad of three microinstructions.
/// The format is as follows:
/// ```plain
///  30 28    25 24 23               8   6       2   0  Index
/// -+---+-----+--|--+----------------+---+-------+---|
///  |CRC|sync | up2 |      uaddr     |up1| eflow |up0|
/// -+---+-----+--|--+----------------+---+-------+---|
///    2   3     2          15          2     4     2   Width
/// ```
///
/// # Fields
/// - `uaddr`: Jump to this address after the instruction with index `up1` has run.
/// - `sync`: Apply some synchronization when running the instruction with index `up2`.
/// - `eflow`: Run the specified control operation after executing the instruction with index `up0`.
/// - `up1`: If `3` don't jump at all. For every other value jump to address `uaddr` after `up1` has run.
/// - `up2`: If `3` don't apply any synchronization. For every other value apply the synchronization at execution of `up2`.
/// - `up0`: Apply the control operation after the execution of `up0`.
/// - `CRC`: Parity CRC
///
/// # Notes
/// - A jump occurs after control operations have run for the specific instruction.
/// - When the instruction after which a jump is scheduled is `TESTUSTATE` or `SUBR`, the jump is taken conditionally.
///
/// # Open questions
/// - How does an `eflow` value like URET/UEND behaves when also a jump is scheduled at other indexes?
///
/// # Sources
///  - https://github.com/chip-red-pill/uCodeDisasm/
///  - https://github.com/pietroborrello/CustomProcessingUnit
///  - https://libmicro.dev/structure.html#sequence-word
#[derive(Default, Clone, Debug)]
pub struct SequenceWord {
    control: Option<SequenceWordPart<SequenceWordControl>>,
    sync: Option<SequenceWordPart<SequenceWordSync>>,
    goto: Option<SequenceWordPart<UCInstructionAddress>>,
}

impl SequenceWord {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_control(&mut self, index: u8, control: SequenceWordControl) -> &mut Self {
        assert!(index < 3);
        self.control = Some(SequenceWordPart {
            apply_to_index: index,
            value: control,
        });
        self
    }

    pub fn no_control(&mut self) -> &mut Self {
        self.control = None;
        self
    }

    pub fn control(&self) -> &Option<SequenceWordPart<SequenceWordControl>> {
        &self.control
    }

    pub fn set_sync(&mut self, index: u8, sync: SequenceWordSync) -> &mut Self {
        assert!(index < 3);
        self.sync = Some(SequenceWordPart {
            apply_to_index: index,
            value: sync,
        });
        self
    }

    pub fn no_sync(&mut self) -> &mut Self {
        self.sync = None;
        self
    }

    pub fn sync(&self) -> &Option<SequenceWordPart<SequenceWordSync>> {
        &self.sync
    }

    pub fn set_goto<T: Into<UCInstructionAddress>>(&mut self, index: u8, goto: T) -> &mut Self {
        assert!(index < 3);
        self.goto = Some(SequenceWordPart {
            apply_to_index: index,
            value: goto.into(),
        });
        self
    }

    pub fn no_goto(&mut self) -> &mut Self {
        self.goto = None;
        self
    }

    pub fn goto(&self) -> &Option<SequenceWordPart<UCInstructionAddress>> {
        &self.goto
    }

    pub fn assemble_no_crc(&self) -> u32 {
        let (tetrad_uidx, tetrad_address) = self
            .goto
            .clone()
            .map(|value| {
                (
                    value.apply_to_index.into(),
                    (value.value.address() & 0x7fff) as u32,
                )
            })
            .unwrap_or((0x3u32, 0x0u32));

        let (sync_uidx, sync_ctrl) = self
            .sync
            .clone()
            .map(|value| (value.apply_to_index.into(), value.value as u32))
            .unwrap_or((0x3u32, 0x0u32));

        let (uop_uidx, uop_ctrl) = self
            .control
            .clone()
            .map(|value| (value.apply_to_index.into(), value.value as u32))
            .unwrap_or((0x0u32, 0x0u32));

        let seqw = (sync_ctrl << 25)
            | (sync_uidx << 23)
            | ((tetrad_address & 0x7fff) << 8)
            | (tetrad_uidx << 6)
            | (uop_ctrl << 2)
            | uop_uidx;

        seqw
    }

    pub fn assemble(&self) -> u32 {
        let seqw = self.assemble_no_crc();
        seqw | even_odd_parity_u32(seqw) << 28
    }

    const MASK: u32 = 0x3fffffff;

    fn check_length(seqw: u32) -> DisassembleResult<()> {
        if (seqw & !Self::MASK) != 0 {
            Err(DisassembleError::InvalidLength(seqw))
        } else {
            Ok(())
        }
    }

    fn check_crc(seqw: u32) -> DisassembleResult<()> {
        let set_crc = (seqw >> 28) & 0b11;
        let sequence_word = seqw & Self::MASK;
        let expected_crc = even_odd_parity_u32(sequence_word);

        if set_crc == expected_crc {
            Ok(())
        } else {
            Err(DisassembleError::InvalidCRC(seqw))
        }
    }

    pub fn disassemble(seqw: u32) -> DisassembleResult<SequenceWord> {
        Self::check_length(seqw)?;
        Self::check_crc(seqw)?;
        Self::disassemble_no_crc_check(seqw)
    }

    pub fn disassemble_no_crc_check(seqw: u32) -> DisassembleResult<SequenceWord> {
        Self::check_length(seqw)?;

        let sync_ctrl = (seqw >> 25) & 0b111;
        let sync_uidx = (seqw >> 23) & 0b11;
        let tetrad_address = (seqw >> 8) & 0x7fff;
        let tetrad_uidx = (seqw >> 6) & 0b11;
        let uop_ctrl = (seqw >> 2) & 0b1111;
        let uop_uidx = seqw & 0b11;

        let goto = if tetrad_address == 0 {
            None
        } else {
            if tetrad_address > 0x7eff {
                return Err(DisassembleError::InvalidGotoAddress(tetrad_address));
            }

            Some(SequenceWordPart {
                apply_to_index: tetrad_uidx as u8,
                value: UCInstructionAddress::from_const(tetrad_address as usize),
            })
        };

        let sync = if sync_uidx == 0x3 || sync_ctrl == 0 {
            if sync_ctrl != 0 {
                return Err(DisassembleError::InvalidSync(sync_uidx, sync_ctrl));
            }

            None
        } else {
            Some(SequenceWordPart {
                apply_to_index: sync_uidx as u8,
                value: SequenceWordSync::from_u32(sync_ctrl).map_or(
                    Err(DisassembleError::InvalidSyncValue(sync_ctrl)),
                    DisassembleResult::Ok,
                )?,
            })
        };

        if uop_uidx == 3 {
            return Err(DisassembleError::InvalidControlIndex(uop_uidx));
        }

        let control = if uop_ctrl == 0 && uop_uidx == 0 {
            None
        } else {
            Some(SequenceWordPart {
                apply_to_index: uop_uidx as u8,
                value: SequenceWordControl::from_u32(uop_ctrl).map_or(
                    Err(DisassembleError::InvalidControlValue(uop_ctrl)),
                    DisassembleResult::Ok,
                )?,
            })
        };

        Ok(SequenceWord {
            control,
            sync,
            goto,
        })
    }
}

pub type DisassembleResult<T> = Result<T, DisassembleError>;
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DisassembleError {
    InvalidLength(u32),
    InvalidCRC(u32),
    InvalidGoto(u32, u32),
    InvalidSync(u32, u32),
    InvalidGotoAddress(u32),
    InvalidControlValue(u32),
    InvalidControlIndex(u32),
    InvalidSyncValue(u32),
}

#[cfg(test)]
mod test {
    use super::*;
    use std::io::BufRead;
    use std::process::Command;

    fn check(label: [&str; 3], seqw: u32) {
        let output = Command::new("python3")
            .arg("../../CustomProcessingUnit/uasm-lib/uasm.py")
            .arg("-d")
            .arg("-s")
            .arg(format!("{:x}", seqw))
            .output()
            .expect("failed to execute process");

        assert!(output.status.success());

        let lines = output
            .stdout
            .lines()
            .map(|line| line.unwrap())
            .collect::<Vec<String>>();

        let regex = regex::Regex::new(r"\s*\[uop[0-2]]\s*").unwrap();

        let uops = lines
            .iter()
            .skip(4)
            .take(3)
            .map(|s| regex.replace_all(s, "").trim().to_string())
            .collect::<Vec<String>>();

        assert_eq!(uops.len(), 3);
        for (a, b) in uops.iter().zip(label.iter()) {
            assert_eq!(a, b);
        }
    }

    #[test]
    fn test_sequencewords() {
        check(["", "", ""], SequenceWord::new().assemble());
        check(
            ["", "SEQW GOTO U7c00", ""],
            SequenceWord::new()
                .set_goto(1, UCInstructionAddress::MSRAM_START)
                .assemble(),
        );
        check(
            ["LFNCEMARK->", "SEQW UEND0", "SEQW GOTO U7c00"],
            SequenceWord::new()
                .set_goto(2, UCInstructionAddress::MSRAM_START)
                .set_sync(0, SequenceWordSync::LFNCEMARK)
                .set_control(1, SequenceWordControl::UEND0)
                .assemble(),
        );
    }

    #[test]
    fn test_disasm_asm_seqw() {
        for seqw in custom_processing_unit::dump::cpu_000506CA::ROM_SEQUENCE.iter() {
            if *seqw == 0 {
                continue;
            }

            let disasm = match SequenceWord::disassemble_no_crc_check(*seqw) {
                Ok(disasm) => disasm,
                Err(err) => panic!("Failed to disassemble: {:x} @ {:?}", seqw, err),
            };
            let asm = disasm.assemble_no_crc();
            assert_eq!(
                *seqw, asm,
                "DISASM -> ASM mismatch: {:x} -> {:x}",
                seqw, asm
            );
        }
    }
}
