pub mod instruction;
pub mod opcodes;
pub mod sequence_word;

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
