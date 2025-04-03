use crate::Trace;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::fmt::Display;
use fuzzer_data::decoder::{InstructionDecoder, InstructionWithBytes};
use iced_x86::{
    BlockEncoder, BlockEncoderOptions, Code, FlowControl, IcedError, Instruction, InstructionBlock,
};
use log::warn;
use rand_core::RngCore;

const FENCE_INSTRUCTIONS: &[&[u8]] = &[
    // lfence
    &[0x0F, 0xAE, 0xE8],
    // mfence
    &[0x0F, 0xAE, 0xF0],
    // sfence
    &[0x0F, 0xAE, 0xF8],
    // nop
    &[0x90],
    // serialize
    // &[0x0F, 0x01, 0xE8]
];

#[derive(Default)]
pub struct Serializer {
    instruction_decoder: InstructionDecoder,
}

#[derive(Debug)]
pub enum SerializeError {
    IcedError(IcedError),
    IndirectBranch,
    Unknown(&'static str),
}

impl Display for SerializeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SerializeError::IcedError(e) => write!(f, "IcedError: {}", e),
            SerializeError::IndirectBranch => {
                write!(f, "Indirect branch instruction cannot be serialized")
            }
            SerializeError::Unknown(e) => write!(f, "Unknown error: {}", e),
        }
    }
}

impl From<IcedError> for SerializeError {
    fn from(value: IcedError) -> Self {
        SerializeError::IcedError(value)
    }
}

impl Serializer {
    pub fn serialize_code<R: RngCore>(
        &mut self,
        random: &mut R,
        code: &[u8],
        trace: &Trace,
    ) -> Result<Vec<u8>, SerializeError> {
        struct Output<'a> {
            pub original_ip: u64,
            pub fence_instruction: &'static [u8],
            pub payload: &'a InstructionWithBytes<'a>,
            pub override_instruction: Option<Vec<u8>>,
        }

        let decoded = self.instruction_decoder.decode(code);

        let mut target_program = Vec::with_capacity(decoded.len() * 2);

        let mut map_old_to_new_ip = BTreeMap::new();

        let mut current_ip = 0;
        for index in 0..decoded.len() {
            let instruction = decoded.get(index).expect("<= len");
            let fence_instruction =
                FENCE_INSTRUCTIONS[random.next_u32() as usize % FENCE_INSTRUCTIONS.len()];

            let fence_ip = current_ip;
            current_ip += fence_instruction.len() as u64;
            let instruction_ip = current_ip;
            current_ip += instruction.bytes.len() as u64;

            let was_executed = trace.was_executed(instruction.instruction.ip());

            let info = InstructionInfo {
                new_ip_serialize_operation: fence_ip,
                new_ip_original_instruction: instruction_ip,
                ip_original_instruction: instruction.instruction.ip(),
                original_instruction_length: instruction.bytes.len() as u64,
                was_executed,
            };

            for l in 0..info.original_instruction_length {
                map_old_to_new_ip.insert(info.ip_original_instruction + l as u64, info);
            }

            target_program.push(Output {
                original_ip: instruction.instruction.ip(),
                fence_instruction,
                payload: instruction,
                override_instruction: None,
            });
        }
        let current_ip = current_ip; // make read-only

        for item in &mut target_program {
            let instruction = &item.payload.instruction;
            let data = map_old_to_new_ip.get(&item.original_ip).unwrap();

            if instruction.is_invalid() {
                continue;
            }

            // paradigma: do best effort for non-executed instructions
            // if an instruction that was executed can not be changed -> abort

            let mut new_instruction = Serializer::patch_ip_rel_memory_oop(instruction)?;

            if new_instruction.is_none() {
                new_instruction = Serializer::patch_special_instructions(
                    code,
                    &map_old_to_new_ip,
                    current_ip,
                    instruction,
                    data,
                )?;
            }

            if new_instruction.is_none() {
                new_instruction = Serializer::patch_control_flow(
                    code,
                    &map_old_to_new_ip,
                    current_ip,
                    instruction,
                    data,
                )?;
            }

            if new_instruction.is_none() {
                continue;
            }
            let new_instruction = new_instruction.unwrap();

            let encoded = BlockEncoder::encode(
                64,
                InstructionBlock::new(&[new_instruction], data.new_ip_original_instruction),
                BlockEncoderOptions::DONT_FIX_BRANCHES,
            );

            match encoded {
                Ok(enc) => {
                    let mut buffer = enc.code_buffer;

                    if buffer.len() < data.original_instruction_length as usize {
                        // prefix was deleted
                        let mut new_buf =
                            Vec::with_capacity(data.original_instruction_length as usize);
                        for _ in 0..(data.original_instruction_length as usize - buffer.len()) {
                            new_buf.push(0x90);
                        }
                        new_buf.extend_from_slice(&buffer);
                        buffer = new_buf;
                    }

                    if buffer.len() == data.original_instruction_length as usize {
                        item.override_instruction = Some(buffer);
                    } else {
                        warn!(
                            "Instruction length mismatch: {} != {}",
                            buffer.len(),
                            data.original_instruction_length
                        );
                        if data.was_executed {
                            return Err(SerializeError::Unknown("Instruction length mismatch"));
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to encode instruction: {}", e);
                    if data.was_executed {
                        return Err(SerializeError::IcedError(e));
                    }
                }
            }
        }

        let mut output = Vec::with_capacity(current_ip as usize);
        for item in &target_program {
            output.extend_from_slice(item.fence_instruction);
            match item.override_instruction {
                Some(ref instr) => {
                    output.extend_from_slice(instr);
                }
                None => {
                    output.extend_from_slice(item.payload.bytes);
                }
            }
        }

        Ok(output)
    }

    fn source_to_target_address(
        target_address_absolute: u64,
        map: &BTreeMap<u64, InstructionInfo>,
        source_end_ip: u64,
        target_end_ip: u64,
    ) -> Result<u64, SerializeError> {
        // todo mid-instruction jumps

        Ok(match map.get(&target_address_absolute) {
            Some(info) => {
                // this address maps to an existing instruction

                if info.ip_original_instruction != target_address_absolute {
                    // this address maps to an existing instruction, but is misaligned
                    return Err(SerializeError::Unknown(
                        "Address maps to an existing instruction, but is misaligned",
                    ));
                }

                info.new_ip_serialize_operation
            }
            None => {
                let address = target_address_absolute % (1 << 30);
                if address >= 7 * 4096 {
                    // point to outside of physical memory

                    -16i64 as u64
                } else if address >= 4096 {
                    // point to outside of code segment

                    // just keep it
                    target_address_absolute
                } else if address >= source_end_ip {
                    // point inside code segment but outside of code

                    // point to an NOP instruction after the end of the code segment
                    target_end_ip.max(address)
                } else {
                    unreachable!("Address points to inside of code segment, but not mapped to an existing instruction: {:X}", address);
                }
            }
        })
    }

    fn patch_ip_rel_memory_oop(
        instruction: &Instruction,
    ) -> Result<Option<Instruction>, SerializeError> {
        if instruction.is_ip_rel_memory_operand() {
            let target_ip = instruction.memory_displacement64();
            let new_target_ip = target_ip;

            let mut instruction = instruction.clone();
            instruction.set_memory_displacement64(new_target_ip);

            Ok(Some(instruction))
        } else {
            Ok(None)
        }
    }

    fn patch_special_instructions(
        _code: &[u8],
        _map_old_to_new_ip: &BTreeMap<u64, InstructionInfo>,
        _current_ip: u64,
        _instruction: &Instruction,
        _data: &InstructionInfo,
    ) -> Result<Option<Instruction>, SerializeError> {
        Ok(None)
    }

    fn patch_control_flow(
        code: &[u8],
        map_old_to_new_ip: &BTreeMap<u64, InstructionInfo>,
        current_ip: u64,
        instruction: &Instruction,
        data: &InstructionInfo,
    ) -> Result<Option<Instruction>, SerializeError> {
        Ok(match instruction.flow_control() {
            FlowControl::Next => None, // dont need to do anything
            FlowControl::UnconditionalBranch => {
                if instruction.is_jmp_near() {
                    // `JMP NEAR relX`
                    let target_ip = instruction.near_branch_target();
                    let new_target_ip = match Serializer::source_to_target_address(
                        target_ip,
                        &map_old_to_new_ip,
                        code.len() as u64,
                        current_ip,
                    ) {
                        Ok(ip) => ip,
                        Err(e) => {
                            if data.was_executed {
                                return Err(e);
                            } else {
                                warn!("Failed to serialize JMP NEAR instruction: {}", e);
                                return Ok(None);
                            }
                        }
                    };

                    let mut instruction = instruction.clone();
                    instruction.set_near_branch64(new_target_ip);

                    Some(instruction)
                } else if instruction.is_jmp_far() {
                    // `JMP FAR ptrXY`
                    let target_ip = instruction.far_branch32();
                    // ignore selector
                    let new_target_ip = match Serializer::source_to_target_address(
                        target_ip as u64,
                        &map_old_to_new_ip,
                        code.len() as u64,
                        current_ip,
                    ) {
                        Ok(ip) => ip,
                        Err(e) => {
                            if data.was_executed {
                                return Err(e);
                            } else {
                                warn!("Failed to serialize JMP FAR instruction: {}", e);
                                return Ok(None);
                            }
                        }
                    };

                    let mut instruction = instruction.clone();
                    instruction.set_far_branch32(new_target_ip as u32); // may produce error?

                    Some(instruction)
                } else if instruction.is_jmp_short() {
                    let target_ip = instruction.memory_displacement64();
                    let new_target_ip = match Serializer::source_to_target_address(
                        target_ip,
                        &map_old_to_new_ip,
                        code.len() as u64,
                        current_ip,
                    ) {
                        Ok(ip) => ip,
                        Err(e) => {
                            if data.was_executed {
                                return Err(e);
                            } else {
                                warn!("Failed to serialize JMP SHORT instruction: {}", e);
                                return Ok(None);
                            }
                        }
                    };

                    let mut instruction = instruction.clone();
                    instruction.set_memory_displacement64(new_target_ip);

                    Some(instruction)
                } else {
                    warn!("Unconditional branch instruction is not a JMP NEAR or JMP FAR");
                    if data.was_executed {
                        return Err(SerializeError::Unknown(
                            "Unconditional branch instruction is not a JMP NEAR or JMP FAR",
                        ));
                    } else {
                        Some(instruction.clone())
                    }
                }
            }
            FlowControl::IndirectBranch | FlowControl::IndirectCall => {
                // `JMP NEAR reg`, `JMP NEAR [mem]`, `JMP FAR [mem]`, `CALL NEAR reg`, `CALL NEAR [mem]`, `CALL FAR [mem]`
                // we cannot handle indirect branches
                if data.was_executed {
                    return Err(SerializeError::IndirectBranch);
                }
                None
            }
            FlowControl::ConditionalBranch => {
                if instruction.is_jcc_short() | instruction.is_loop() | instruction.is_loopcc()
                    // todo report this bug to rust iced-x86 crate
                | (instruction.code() >= Code::Jecxz_rel8_16 && instruction.code() <= Code::Jrcxz_rel8_64)
                {
                    // `Jcc SHORT relX`, `LOOP rel8`, `LOOPcc rel8`
                    let target_ip = instruction.memory_displacement64();
                    let new_target_ip = match Serializer::source_to_target_address(
                        target_ip as u64,
                        &map_old_to_new_ip,
                        code.len() as u64,
                        current_ip,
                    ) {
                        Ok(ip) => ip,
                        Err(e) => {
                            if data.was_executed {
                                return Err(e);
                            } else {
                                warn!("Failed to serialize conditional branch instruction: {}", e);
                                return Ok(None);
                            }
                        }
                    };

                    let mut instruction = instruction.clone();
                    instruction.set_memory_displacement64(new_target_ip);

                    Some(instruction)
                } else if instruction.is_jcc_near() {
                    // `Jcc NEAR`
                    let target_ip = instruction.near_branch_target();
                    let new_target_ip = match Serializer::source_to_target_address(
                        target_ip,
                        &map_old_to_new_ip,
                        code.len() as u64,
                        current_ip,
                    ) {
                        Ok(ip) => ip,
                        Err(e) => {
                            if data.was_executed {
                                return Err(e);
                            } else {
                                warn!("Failed to serialize conditional branch instruction: {}", e);
                                return Ok(None);
                            }
                        }
                    };

                    let mut instruction = instruction.clone();
                    instruction.set_near_branch64(new_target_ip);

                    Some(instruction)
                } else {
                    warn!("Conditional branch instruction is not a Jcc SHORT, Jcc NEAR, LOOP, or LOOPcc");
                    if data.was_executed {
                        return Err(SerializeError::Unknown("Conditional branch instruction is not a Jcc SHORT, Jcc NEAR, LOOP, or LOOPcc"));
                    } else {
                        Some(instruction.clone())
                    }
                }
            }
            FlowControl::Return
            | FlowControl::Interrupt
            | FlowControl::XbeginXabortXend
            | FlowControl::Exception => {
                // return address is on stack
                // interrupt will terminate VMX
                // XbeginXabortXend does not change addresses
                // exception will terminate VMX
                None
            }
            FlowControl::Call => {
                if instruction.is_call_near() {
                    // `CALL NEAR relX`
                    let target_ip = instruction.near_branch_target();
                    let new_target_ip = match Serializer::source_to_target_address(
                        target_ip,
                        &map_old_to_new_ip,
                        code.len() as u64,
                        current_ip,
                    ) {
                        Ok(ip) => ip,
                        Err(e) => {
                            if data.was_executed {
                                return Err(e);
                            } else {
                                warn!("Failed to serialize CALL NEAR instruction: {}", e);
                                return Ok(None);
                            }
                        }
                    };

                    let mut instruction = instruction.clone();
                    instruction.set_near_branch64(new_target_ip);

                    Some(instruction)
                } else if instruction.is_call_far() {
                    // `CALL FAR ptrXY`
                    let target_ip = instruction.far_branch32();
                    // ignore selector
                    let new_target_ip = match Serializer::source_to_target_address(
                        target_ip as u64,
                        &map_old_to_new_ip,
                        code.len() as u64,
                        current_ip,
                    ) {
                        Ok(ip) => ip,
                        Err(e) => {
                            if data.was_executed {
                                return Err(e);
                            } else {
                                warn!("Failed to serialize CALL FAR instruction: {}", e);
                                return Ok(None);
                            }
                        }
                    };

                    let mut instruction = instruction.clone();
                    instruction.set_far_branch32(new_target_ip as u32); // may produce error?

                    Some(instruction)
                } else {
                    // other call instructions will terminate VMX
                    None
                }
            }
        })
    }
}

#[derive(Clone, Copy)]
struct InstructionInfo {
    pub ip_original_instruction: u64,
    pub original_instruction_length: u64,
    pub new_ip_serialize_operation: u64,
    pub new_ip_original_instruction: u64,
    pub was_executed: bool,
}

#[cfg(test)]
mod test {
    use crate::mutation_engine::serialize::Serializer;
    use crate::{disassemble_code, Trace};
    use iced_x86::code_asm;
    use iced_x86::code_asm::CodeAssembler;
    use rand_core::SeedableRng;
    use rand_isaac::isaac64;
    use std::println;
    use uefi::println;

    #[test]
    fn test_serialize() {
        let mut serializer = Serializer::default();
        let mut rnd = &mut isaac64::Isaac64Rng::seed_from_u64(0);

        let mut code = {
            let mut assembler = CodeAssembler::new(64).unwrap();

            let mut label = assembler.create_label();

            assembler.mov(code_asm::rax, 0x1234u64).unwrap();
            assembler.set_label(&mut label).unwrap();
            assembler.jmp(label).unwrap();

            assembler.nop().unwrap();

            assembler.jns(0x38).unwrap();

            assembler.assemble(0).unwrap()
        };

        code.clear();
        code.extend_from_slice(&[0x9E, 0x97, 0x5D, 0x55, 0x39, 0xD2, 0xE0, 0x8E, 0x50, 0x0F]);

        disassemble_code(&code);

        let mut trace = Trace::default();
        for i in 0..code.len() {
            trace.push(i as u64);
        }
        let serialized = serializer.serialize_code(&mut rnd, &code, &trace).unwrap();

        println!();

        disassemble_code(&serialized);

        println!("Original: {:X?}", code);
        println!("Serialized: {:X?}", serialized);
    }
}
