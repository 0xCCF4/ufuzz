use crate::even_odd_parity_u64;
use crate::opcodes::Opcode;
use core::fmt::Display;
use data_types::addresses::{Address, UCInstructionAddress};
use num_traits::FromPrimitive;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(transparent)]
pub struct Instruction {
    instruction: u64,
}

impl Instruction {
    pub const NOP: Instruction = Instruction::disassemble(0);

    pub const fn disassemble(micro_operation: u64) -> Instruction {
        Instruction {
            instruction: micro_operation & 0x3FFFFFFFFFFF, // cut of CRC
        }
    }

    pub const fn from_opcode(opcode: Opcode) -> Instruction {
        Self {
            instruction: (opcode as u16 as u64) << 32,
        }
    }

    const fn opcode_raw(&self) -> u16 {
        ((self.instruction >> 32) & 0xFFF) as u16
    }

    pub fn opcode(&self) -> Opcode {
        Opcode::from_u16(self.opcode_raw())
            .expect("Since OpcodeEnum contains all possible values, this should never fail")
    }

    pub fn assemble_no_crc(&self) -> u64 {
        self.instruction
    }

    pub fn assemble(&self) -> u64 {
        let instruction = self.assemble_no_crc();
        instruction | even_odd_parity_u64(instruction) << 46
    }

    const fn immediate_src1_encode(src1_imm: u64) -> u64 {
        (((src1_imm) & 0xff) << 24)
            | (((src1_imm) & 0x1f00) << 10)
            | (((src1_imm) & 0xe000) >> 7)
            | (1 << 9)
    }

    const fn opcode_encode(opcode: Opcode) -> u64 {
        (opcode as u64) << 32
    }

    #[allow(non_snake_case)]
    pub fn UJMP<A: Into<UCInstructionAddress>>(address: A) -> Instruction {
        let value = Self::opcode_encode(Opcode::UJMP)
            | Self::immediate_src1_encode(address.into().address() as u64);
        Instruction::disassemble(value)
    }
}

impl Display for Instruction {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.instruction == 0 {
            write!(f, "NOP")
        } else {
            write!(f, "{}", self.opcode())
        }
    }
}

impl From<u64> for Instruction {
    fn from(instruction: u64) -> Self {
        Instruction::disassemble(instruction)
    }
}

// todo: add further disassembly/assembly methods

// todo: add tests
