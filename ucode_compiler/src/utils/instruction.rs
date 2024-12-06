pub struct Instruction {
    instruction: u64,
}

impl Instruction {
    pub fn disassemble(micro_operation: u64) -> Instruction {
        Instruction {
            instruction: micro_operation,
        }
    }

    pub fn opcode(&self) -> u16 {
        ((self.instruction >> 32) & 0xFFF) as u16
    }
}

// todo: add further disassembly/assembly methods
