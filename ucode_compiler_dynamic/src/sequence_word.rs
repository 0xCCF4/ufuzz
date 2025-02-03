use crate::even_odd_parity_u32;
use core::fmt::Display;
use data_types::addresses::{Address, UCInstructionAddress};
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;

use alloc::format;
use alloc::vec::Vec;
use core::borrow::{Borrow, BorrowMut};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, FromPrimitive)]
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

impl SequenceWordControl {
    pub fn is_uend(&self) -> bool {
        matches!(
            self,
            SequenceWordControl::UEND0
                | SequenceWordControl::UEND1
                | SequenceWordControl::UEND2
                | SequenceWordControl::UEND3
        )
    }

    pub fn is_uret(&self) -> bool {
        matches!(
            self,
            SequenceWordControl::URET0 | SequenceWordControl::URET1
        )
    }

    pub fn is_terminator(&self) -> bool {
        self.is_uend() || self.is_uret()
    }

    pub fn is_saveupip(&self) -> bool {
        matches!(
            self,
            SequenceWordControl::SAVEUPIP0
                | SequenceWordControl::SAVEUPIP1
                | SequenceWordControl::ROVR_SAVEUPIP0
                | SequenceWordControl::ROVR_SAVEUPIP1
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, FromPrimitive)]
pub enum SequenceWordSync {
    LFNCEWAIT = 0x1,
    LFNCEMARK = 0x2,
    LFNCEWTMRK = 0x3,
    SYNCFULL = 0x4,
    SYNCWAIT = 0x5,
    SYNCMARK = 0x6,
    SYNCWTMRK = 0x7,
}

#[derive(Clone, Debug, Copy, PartialEq, Eq, Hash)]
pub struct SequenceWordPart<T> {
    pub apply_to_index: u8,
    pub value: T,
}

/// A sequence word is a modifier applied to a triad of three microinstructions.
/// The format is as follows:
/// ```plain
///  30 28    25 24 23                8   6       2   0 Index
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
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

    pub fn apply_control(mut self, index: u8, control: SequenceWordControl) -> Self {
        self.set_control(index, control);
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

    pub fn apply_sync(mut self, index: u8, sync: SequenceWordSync) -> Self {
        self.set_sync(index, sync);
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

    pub fn apply_goto<T: Into<UCInstructionAddress>>(mut self, index: u8, goto: T) -> Self {
        self.set_goto(index, goto);
        self
    }

    pub fn no_goto(&mut self) -> &mut Self {
        self.goto = None;
        self
    }

    pub fn goto(&self) -> &Option<SequenceWordPart<UCInstructionAddress>> {
        &self.goto
    }

    pub fn assemble_no_crc(&self) -> AssembleResult<u32> {
        if !self.is_valid() {
            return Err(AssembleError::InvalidCombination);
        }

        let (tetrad_uidx, tetrad_address) = self
            .goto
            .map(|value| {
                (
                    value.apply_to_index.into(),
                    (value.value.address() & 0x7fff) as u32,
                )
            })
            .unwrap_or((0x3u32, 0x0u32));

        let (sync_uidx, sync_ctrl) = self
            .sync
            .map(|value| (value.apply_to_index.into(), value.value as u32))
            .unwrap_or((0x3u32, 0x0u32));

        let (uop_uidx, uop_ctrl) = self
            .control
            .map(|value| (value.apply_to_index.into(), value.value as u32))
            .unwrap_or((0x0u32, 0x0u32));

        Ok((sync_ctrl << 25)
            | (sync_uidx << 23)
            | ((tetrad_address & 0x7fff) << 8)
            | (tetrad_uidx << 6)
            | (uop_ctrl << 2)
            | uop_uidx)
    }

    pub fn assemble(&self) -> AssembleResult<u32> {
        let seqw = self.assemble_no_crc()?;
        Ok(seqw | even_odd_parity_u32(seqw) << 28)
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
        let sequence_word = seqw & (Self::MASK >> 2);
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
                value: SequenceWordSync::from_u32(sync_ctrl)
                    .ok_or(DisassembleError::InvalidSyncValue(sync_ctrl))?,
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
                value: SequenceWordControl::from_u32(uop_ctrl)
                    .ok_or(DisassembleError::InvalidControlValue(uop_ctrl))?,
            })
        };

        Ok(SequenceWord {
            control,
            sync,
            goto,
        })
    }

    pub fn is_valid(&self) -> bool {
        if let Some(control) = self.control() {
            if let Some(goto) = self.goto() {
                if (control.value.is_uret() || control.value.is_uend())
                    && control.apply_to_index == goto.apply_to_index
                {
                    return false;
                }
            }
        }
        true
    }

    pub fn view(&self, base: u8, len: u8) -> SequenceWordView<&SequenceWord> {
        SequenceWordView::new(self, base, len)
    }

    pub fn view_mut(&mut self, base: u8, len: u8) -> SequenceWordView<&mut SequenceWord> {
        SequenceWordView::new(self, base, len)
    }

    pub fn apply_view(self, base: u8, len: u8) -> SequenceWordView<SequenceWord> {
        SequenceWordView::new(self, base, len)
    }

    pub fn apply_shift(mut self, amount: i8) -> Self {
        self.shift(amount);
        self
    }

    pub fn shift(&mut self, amount: i8) -> &mut Self {
        if amount == 0 {
            return self;
        }

        if let Some(control) = self.control() {
            let target = control.apply_to_index as i8 + amount;
            if !(0..=2).contains(&target) {
                self.no_control();
            } else {
                self.set_control(target as u8, control.value);
            }
        }

        if let Some(sync) = self.sync() {
            let target = sync.apply_to_index as i8 + amount;
            if !(0..=2).contains(&target) {
                self.no_sync();
            } else {
                self.set_sync(target as u8, sync.value);
            }
        }

        if let Some(goto) = self.goto() {
            let target = goto.apply_to_index as i8 + amount;
            if !(0..=2).contains(&target) {
                self.no_goto();
            } else {
                self.set_goto(target as u8, goto.value);
            }
        }

        self
    }
}

impl Display for SequenceWord {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut text = [Vec::new(), Vec::new(), Vec::new()];
        if let Some(control) = &self.control {
            text[control.apply_to_index as usize].push(format!("{:?}", control.value));
        }
        if let Some(sync) = &self.sync {
            text[sync.apply_to_index as usize].push(format!("{:?}", sync.value));
        }
        if let Some(goto) = &self.goto {
            text[goto.apply_to_index as usize].push(format!("GOTO {:?}", goto.value));
        }

        write!(f, "[{}]", text.map(|v| v.join(" ")).join(","))
    }
}

pub type DisassembleResult<T> = Result<T, DisassembleError>;
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
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
pub type AssembleResult<T> = Result<T, AssembleError>;
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum AssembleError {
    InvalidCombination,
}

pub struct SequenceWordView<T: Borrow<SequenceWord>> {
    word: T,
    base: u8,
    len: u8,
}

impl<T: Borrow<SequenceWord>> SequenceWordView<T> {
    fn new(word: T, base: u8, len: u8) -> Self {
        Self { word, base, len }
    }

    fn rebase<V: Copy>(&self, value: &Option<SequenceWordPart<V>>) -> Option<SequenceWordPart<V>> {
        match value {
            Some(control)
                if control.apply_to_index >= self.base
                    && control.apply_to_index < self.base + self.len =>
            {
                Some(SequenceWordPart {
                    apply_to_index: control.apply_to_index - self.base,
                    value: control.value,
                })
            }
            _ => None,
        }
    }

    pub fn control(&self) -> Option<SequenceWordPart<SequenceWordControl>> {
        self.rebase(self.word.borrow().control())
    }

    pub fn sync(&self) -> Option<SequenceWordPart<SequenceWordSync>> {
        self.rebase(self.word.borrow().sync())
    }

    pub fn goto(&self) -> Option<SequenceWordPart<UCInstructionAddress>> {
        self.rebase(self.word.borrow().goto())
    }

    pub fn close(self) -> T {
        self.word
    }
}

impl<T: BorrowMut<SequenceWord>> SequenceWordView<T> {
    pub fn set_control(&mut self, index: u8, control: SequenceWordControl) -> &mut Self {
        assert!(index < self.len);
        self.word
            .borrow_mut()
            .set_control(index + self.base, control);
        self
    }

    pub fn no_control(&mut self) -> &mut Self {
        self.word.borrow_mut().no_control();
        self
    }

    pub fn set_sync(&mut self, index: u8, sync: SequenceWordSync) -> &mut Self {
        assert!(index < self.len);
        self.word.borrow_mut().set_sync(index + self.base, sync);
        self
    }

    pub fn no_sync(&mut self) -> &mut Self {
        self.word.borrow_mut().no_sync();
        self
    }

    pub fn set_goto<A: Into<UCInstructionAddress>>(&mut self, index: u8, goto: A) -> &mut Self {
        assert!(index < self.len);
        self.word.borrow_mut().set_goto(index + self.base, goto);
        self
    }

    pub fn no_goto(&mut self) -> &mut Self {
        self.word.borrow_mut().no_goto();
        self
    }

    pub fn cleanup_outside(&mut self) -> &mut Self {
        if self.control().is_none() {
            self.word.borrow_mut().no_control();
        }
        if self.sync().is_none() {
            self.word.borrow_mut().no_sync();
        }
        if self.goto().is_none() {
            self.word.borrow_mut().no_goto();
        }
        self
    }

    pub fn apply(mut self) -> T {
        self.cleanup_outside();
        self.word.borrow_mut().shift(-(self.base as i8));
        self.word
    }
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
        check(["", "", ""], SequenceWord::new().assemble().unwrap());
        check(
            ["", "SEQW GOTO U7c00", ""],
            SequenceWord::new()
                .set_goto(1, UCInstructionAddress::MSRAM_START)
                .assemble()
                .unwrap(),
        );
        check(
            ["LFNCEMARK->", "SEQW UEND0", "SEQW GOTO U7c00"],
            SequenceWord::new()
                .set_goto(2, UCInstructionAddress::MSRAM_START)
                .set_sync(0, SequenceWordSync::LFNCEMARK)
                .set_control(1, SequenceWordControl::UEND0)
                .assemble()
                .unwrap(),
        );
    }

    #[test]
    fn test_disasm_asm_seqw() {
        for seqw in ucode_dump::dump::cpu_000506CA::ROM_SEQUENCE.iter() {
            if *seqw == 0 {
                continue;
            }

            let disasm = match SequenceWord::disassemble_no_crc_check(*seqw) {
                Ok(disasm) => disasm,
                Err(err) => panic!("Failed to disassemble: {:x} @ {:?}", seqw, err),
            };
            let asm = disasm.assemble_no_crc().unwrap();
            assert_eq!(
                *seqw, asm,
                "DISASM -> ASM mismatch: {:x} -> {:x}",
                seqw, asm
            );
        }
    }

    #[test]
    fn test_seq_manual() -> Result<(), DisassembleError> {
        let rom = custom_processing_unit::dump::ROM_cpu_000506CA;
        let hooked_address = UCInstructionAddress::from_const(0x428);

        let input = rom
            .get_sequence_word(hooked_address)
            .expect("Failed to get seqw");
        println!("Original Bits: {:x}", input);

        let mut word = SequenceWord::disassemble_no_crc_check(input)?;
        println!("Original: {}", word);

        word.set_goto(hooked_address.triad_offset(), hooked_address.next_address());

        println!("{}", word);

        assert_eq!(input, 0x0199c980);
        assert_eq!(word.assemble(), 0x31842900);

        Ok(())
    }
}
