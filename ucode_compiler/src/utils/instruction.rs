use crate::utils::even_odd_parity_u64;
use crate::utils::opcodes::Opcode;
use num_traits::FromPrimitive;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
}

impl From<u64> for Instruction {
    fn from(instruction: u64) -> Self {
        Instruction::disassemble(instruction)
    }
}

// todo: add further disassembly/assembly methods

// todo: add tests
