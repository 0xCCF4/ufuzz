#![no_std]

extern crate alloc;
extern crate core;

use crate::instruction::Instruction;
use crate::opcodes::Opcode;
use crate::sequence_word::{DisassembleError, SequenceWord};
use core::fmt;
use core::fmt::{Display, Formatter};
use data_types::addresses::UCInstructionAddress;

pub mod instruction;
pub mod opcodes;
pub mod sequence_word;

impl Display for opcodes::Opcode {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

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

impl TryFrom<data_types::patch::Triad> for Triad {
    type Error = DisassembleError;
    fn try_from(value: data_types::patch::Triad) -> Result<Self, Self::Error> {
        Ok(Triad {
            instructions: [
                Instruction::disassemble(value.instructions[0]),
                Instruction::disassemble(value.instructions[1]),
                Instruction::disassemble(value.instructions[2]),
            ],
            sequence_word: SequenceWord::disassemble_no_crc_check(value.sequence_word)?,
        })
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

pub enum ControlFlowNextMode {
    /// Execution of next instruction ip+1
    Direct,
    /// Execution of next instruction SEQW GOTO
    Indirect,
}

pub enum ControlFlowBranchPrediction {
    Taken,
    NotTaken,
    NotPredicted,
}

pub enum ControlFlow {
    /// The next instruction is executed
    Next {
        mode: ControlFlowNextMode,
        target: UCInstructionAddress,
    },

    ConditionalBranch {
        prediction: ControlFlowBranchPrediction,
        taken: Option<UCInstructionAddress>, // set if immediate
        not_taken: UCInstructionAddress,
    },
    UnconditionalBranch {
        prediction: ControlFlowBranchPrediction,
        next: Option<UCInstructionAddress>, // set if immediate
    },

    Unknown,
}

impl Opcode {
    pub fn control_flow(&self) -> ControlFlow {
        todo!() // todo, also see "Analyzing and Exploiting Branch Mispredictions in Microcode"
    }
}
