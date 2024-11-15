use data_types::addresses::{Address, UCInstructionAddress};

// output parity1 || parity0
pub fn parity(mut value: u32) -> u32 {
    let mut result = 0;
    while value > 0 {
        result ^= value & 3;
        value >>= 2;
    }
    result
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SequenceWordControl {
    URET0 = 0x2,
    URET1 = 0x3,
    UEND0 = 0xC,
    UEND1 = 0xD,
    UEND2 = 0xE,
    UEND3 = 0xF,
    WRTAGW = 0x8,
    MSLOOP = 0x9,
    MSSTOP = 0xB,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
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
    apply_to_index: u8,
    value: T,
}

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

    pub fn control(&mut self, index: u8, control: SequenceWordControl) -> &mut Self {
        assert!(index < 3);
        self.control = Some(SequenceWordPart {
            apply_to_index: index,
            value: control,
        });
        self
    }

    pub fn sync(&mut self, index: u8, sync: SequenceWordSync) -> &mut Self {
        assert!(index < 3);
        self.sync = Some(SequenceWordPart {
            apply_to_index: index,
            value: sync,
        });
        self
    }

    pub fn goto<T: Into<UCInstructionAddress>>(&mut self, index: u8, goto: T) -> &mut Self {
        assert!(index < 3);
        self.goto = Some(SequenceWordPart {
            apply_to_index: index,
            value: goto.into(),
        });
        self
    }

    pub fn assemble(&self) -> u32 {
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

        seqw | parity(seqw) << 28
    }
}

#[cfg(test)]
mod test {
    use crate::utils::SequenceWord;
    use crate::utils::SequenceWordControl::UEND0;
    use crate::utils::SequenceWordSync::{LFNCEMARK};
    use data_types::addresses::UCInstructionAddress;
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
                .goto(1, UCInstructionAddress::MSRAM_START)
                .assemble(),
        );
        check(
            ["LFNCEMARK->", "SEQW UEND0", "SEQW GOTO U7c00"],
            SequenceWord::new()
                .goto(2, UCInstructionAddress::MSRAM_START)
                .sync(0, LFNCEMARK)
                .control(1, UEND0)
                .assemble(),
        );
    }
}
