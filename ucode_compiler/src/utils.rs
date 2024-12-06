pub mod instruction;
pub mod opcodes;
pub mod sequence_word;

// output parity1 || parity0
pub fn even_odd_parity(mut value: u32) -> u32 {
    let mut result = 0;
    while value > 0 {
        result ^= value & 3;
        value >>= 2;
    }
    result
}
