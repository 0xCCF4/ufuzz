use crate::database::Database;
use crate::device_connection::DeviceConnection;
use crate::fuzzer_node_bridge::FuzzerNodeInterface;
use crate::genetic_breeding::{net_blacklist, ExecuteSampleResult};
use crate::CommandExitResult;
use fuzzer_data::OtaC2DTransport;
use iced_x86::{Decoder, DecoderOptions, Formatter, Instruction, NasmFormatter};
use itertools::Itertools;
use log::error;
use std::io;
use std::io::Write;
use std::path::Path;

pub async fn main<A: AsRef<Path>, B: AsRef<Path>>(
    udp: &mut DeviceConnection,
    interface: &FuzzerNodeInterface,
    db: &mut Database,
    input: A,
    output: B,
    overwrite: bool,
) -> CommandExitResult {
    if !input.as_ref().exists() {
        eprintln!("Input file does not exist");
        return CommandExitResult::ExitProgram;
    }

    if output.as_ref().exists() && !overwrite {
        print!("Output file already exists. Overwrite? (y/n): ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        if input.trim().to_lowercase() != "y" {
            return CommandExitResult::ExitProgram;
        }
    }

    let input_buf = match std::fs::read(input) {
        Ok(buf) => buf,
        Err(e) => {
            eprintln!("Failed to read input file: {}", e);
            return CommandExitResult::ExitProgram;
        }
    };
    let input_buf = match String::from_utf8(input_buf) {
        Ok(buf) => buf,
        Err(e) => {
            eprintln!("Input file is not valid UTF-8: {}", e);
            return CommandExitResult::ExitProgram;
        }
    }
    .replace(|c: char| !c.is_ascii_hexdigit(), "");

    let actual_bytes = (0..input_buf.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&input_buf[i..i + 2], 16).unwrap())
        .collect::<Vec<u8>>();

    if actual_bytes.len() > 100 {
        eprintln!("Input file too large");
        return CommandExitResult::ExitProgram;
    }

    if !net_blacklist(udp, db).await {
        return CommandExitResult::RetryOrReconnect;
    }

    let (mut result, events) = match crate::genetic_breeding::net_execute_sample(
        udp,
        interface,
        db,
        &actual_bytes,
    )
    .await
    {
        ExecuteSampleResult::Timeout => return CommandExitResult::RetryOrReconnect,
        ExecuteSampleResult::Rerun => return CommandExitResult::Operational,
        ExecuteSampleResult::Success(a, b) => (a, b),
    };

    db.push_results(actual_bytes.to_vec(), result.clone(), events.clone(), 0, 0);

    let mut output_text = "Disassembly:\n".to_string();
    disassemble_code(&actual_bytes, &mut output_text);

    if let Some(serialized) = result.serialized.as_ref() {
        output_text.push_str("\n\nSerialized:\n");
        disassemble_code(&serialized, &mut output_text);
    }

    let count_very_like_bug = events
        .iter()
        .filter(|e| matches!(e, fuzzer_data::ReportExecutionProblem::VeryLikelyBug { .. }))
        .count();
    let count_serialized_mismatch = events
        .iter()
        .filter(|e| {
            matches!(
                e,
                fuzzer_data::ReportExecutionProblem::SerializedMismatch { .. }
            )
        })
        .count();
    let count_trace_mismatch = events
        .iter()
        .filter(|e| {
            matches!(
                e,
                fuzzer_data::ReportExecutionProblem::StateTraceMismatch { .. }
            )
        })
        .count();
    let count_coverage_mismatch = events
        .iter()
        .filter(|e| {
            matches!(
                e,
                fuzzer_data::ReportExecutionProblem::CoverageProblem { .. }
            )
        })
        .count();
    let other = events.len()
        - count_very_like_bug
        - count_serialized_mismatch
        - count_trace_mismatch
        - count_coverage_mismatch;

    println!("\nEvent summary:");
    println!("  - Very likely bug: {}", count_very_like_bug);
    println!("  - Serialized mismatch: {}", count_serialized_mismatch);
    println!("  - State trace mismatch: {}", count_trace_mismatch);
    println!("  - Coverage mismatch: {}", count_coverage_mismatch);
    println!("  - Other: {}", other);

    output_text.push_str("\n\nExit result:\n");
    output_text.push_str(&format!(
        "{}",
        serde_json::to_string_pretty(&result).unwrap()
    ));
    output_text.push_str("\nEND-EXIT\n");

    output_text.push_str("\n\nEvents:\n");
    output_text.push_str(&format!(
        "{}",
        serde_json::to_string_pretty(&events).unwrap()
    ));
    output_text.push_str("\nEND-EVENTS\n");

    println!("Coverage:");
    for c in result.coverage {
        println!(" - {:04x}: {:04x}", c.0, c.1);
    }

    let _ = db.save().await.map_err(|e| {
        error!("Failed to save the database: {:?}", e);
    });

    if let Err(e) = std::fs::write(output, output_text) {
        eprintln!("Failed to write output file: {}", e);
        return CommandExitResult::ExitProgram;
    }

    CommandExitResult::ExitProgram
}

pub fn disassemble_code(code: &[u8], out: &mut String) {
    let mut decoder = Decoder::with_ip(64, code, 0, DecoderOptions::NONE);
    let mut formatter = NasmFormatter::new();

    formatter.options_mut().set_digit_separator("`");
    formatter.options_mut().set_first_operand_char_index(10);
    formatter.options_mut().set_show_useless_prefixes(true);

    let mut output = String::new();
    let mut instruction = Instruction::default();

    while decoder.can_decode() {
        decoder.decode_out(&mut instruction);
        output.clear();
        formatter.format(&instruction, &mut output);

        // Eg. "00007FFAC46ACDB2 488DAC2400FFFFFF     lea       rbp,[rsp-100h]"
        out.push_str(&format!("{:016X} ", instruction.ip()));
        let start_index = (instruction.ip() - 0) as usize;
        let instr_bytes = &code[start_index..start_index + instruction.len()];
        for b in instr_bytes.iter() {
            out.push_str(&format!("{:02X}", b));
        }
        if instr_bytes.len() < 10 {
            for _ in 0..10 - instr_bytes.len() {
                out.push_str("  ");
            }
        }
        out.push_str(&format!(" {}\n", output));
    }
}
