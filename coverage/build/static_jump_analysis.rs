use std::collections::BTreeMap;
use std::fs;
use itertools::Itertools;
use ucode_compiler::utils::sequence_word::SequenceWord;
use ucode_dump::dump::{ROMS_DISASM};
use ucode_dump::{dump, RomDump};
use data_types::addresses::{Address, UCInstructionAddress};


#[allow(dead_code)]
pub fn jump_analysis() {
    analyse_jumps(&dump::ROM_cpu_000506CA); // TODO add other archs
}

fn analyse_jumps(rom: &RomDump<'static, 'static>) {
    // address bases -> list ( address base, jumping towards the key )
    let mut jump_towards: BTreeMap<UCInstructionAddress, Vec<UCInstructionAddress>> = BTreeMap::new();

    for (i, sequence_word) in rom.sequence_words().iter().enumerate() {
        let sequence_word = SequenceWord::disassemble_no_crc_check(*sequence_word).expect("Failed to disassemble sequence word");

        if let Some(goto) = sequence_word.goto() {
            jump_towards.entry(goto.value).or_default().push(UCInstructionAddress::from_const(i * 4).with_triad_offset(goto.apply_to_index));
        }
    }

    println!("Total sequence word jump targets: {}", jump_towards.len());
    println!("Total odd sequence word jump targets: {}", jump_towards.iter().filter(|x| x.0.is_odd()).count());


    // hacky, but need to get the job done fast
    let regex_find_address = regex::Regex::new("U([0-9a-fA-F]{4})").unwrap();
    let regex_find_goto = regex::Regex::new("GOTO U([0-9a-fA-F]{4})").unwrap();
    let dissassembly = ROMS_DISASM.iter().find(|x| x.model() == rom.model()).unwrap().dissassembly();
    for line in dissassembly.lines() {
        if let Some((address, content)) = line.split_once(": ") {
            let address = usize::from_str_radix(&address[1..], 16).unwrap();
            let content_no_goto = regex_find_goto.replace_all(content.trim(), "GOTO X$1").to_string();

            for capture in regex_find_address.captures_iter(&content_no_goto) {
                let goto_address = usize::from_str_radix(&capture.get(1).unwrap().as_str(), 16).unwrap();
                jump_towards.entry(UCInstructionAddress::from_const(goto_address)).or_default().push(UCInstructionAddress::from_const(address));
            }
        }
    }

    println!("Total jumps: {}", jump_towards.len());

    let odd_targets = jump_towards.iter().filter(|x| x.0.is_odd()).collect_vec();

    println!("Total: {}", jump_towards.len());
    println!("Event targets: {}", jump_towards.iter().filter(|x| x.0.is_even()).count());
    println!("Odd targets: {}", odd_targets.len());

    let mut blacklist = String::new();

    blacklist.push_str("[\n");

    blacklist.push_str(std::fs::read_to_string("src/blacklist.txt").expect("unable to read blacklist").as_str());
    blacklist.push_str("\n\n");

    for x in &odd_targets {
        let target = x.0;
        let target_base = target.align_even();
        let from = x.1.iter().map(|v|v.address()).collect_vec();
        blacklist.push_str(&format!("0x{:04x}, // excluded because maybe odd const jump from {:04x?} towards {}\n", target_base.address(), from, target));
    }
    blacklist.push_str("]\n");

    fs::write("src/blacklist.gen", blacklist).expect("unable to write blacklist");
}
