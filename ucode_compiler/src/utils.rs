use crate::utils::instruction::Instruction;
use crate::utils::sequence_word::SequenceWord;
use core::fmt::{Display, Formatter};

pub mod instruction;
pub mod opcodes;
pub mod sequence_word;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Triad {
    pub instructions: [Instruction; 3],
    pub sequence_word: SequenceWord,
}

impl Triad {
    pub fn assemble(&self) -> sequence_word::AssembleResult<[u64; 4]> {
        Ok([
            self.instructions[0].assemble(),
            self.instructions[1].assemble(),
            self.instructions[2].assemble(),
            self.sequence_word.assemble()? as u64,
        ])
    }
}

impl Display for Triad {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{} {} {} {}",
            self.instructions[0], self.instructions[1], self.instructions[2], self.sequence_word
        )
    }
}

pub fn even_odd_parity_u32(mut value: u32) -> u32 {
    let mut result = 0;
    while value > 0 {
        result ^= value & 3;
        value >>= 2;
    }
    result
}

pub fn even_odd_parity_u64(mut value: u64) -> u64 {
    let mut result = 0;
    while value > 0 {
        result ^= value & 3;
        value >>= 2;
    }
    result
}
