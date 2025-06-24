//! Manual Execution Module
//!
//! This module provides functionality for manually executing code samples and analyzing
//! their results.

use crate::database::Database;
use crate::device_connection::DeviceConnection;
use crate::fuzzer_node_bridge::FuzzerNodeInterface;
use crate::net::{net_execute_sample, net_fuzzing_pretext, ExecuteSampleResult};
use crate::CommandExitResult;
use fuzzer_data::ReportExecutionProblem;
use hypervisor::state::StateDifference;
use iced_x86::{Decoder, DecoderOptions, Formatter, Instruction, NasmFormatter};
use log::{error, info};
use std::collections::VecDeque;
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};

/// State of manual execution process
#[derive(Debug, Default)]
pub enum ManualExecutionState {
    /// Execution has not started
    #[default]
    NotStarted,
    /// Execution is in progress
    Started {
        /// Optional output file path
        output: Option<PathBuf>,
        /// Queue of samples to execute
        samples: VecDeque<Vec<u8>>,
    },
}

/// Main entry point for manual execution
///
/// # Returns
///
/// * `CommandExitResult` indicating the outcome of execution
pub async fn main<A: AsRef<Path>, B: AsRef<Path>>(
    udp: &mut DeviceConnection,
    interface: &FuzzerNodeInterface,
    db: &mut Database,
    input: A,
    output: Option<B>,
    overwrite: bool,
    bulk: bool,
    state: &mut ManualExecutionState,
) -> CommandExitResult {
    if matches!(state, ManualExecutionState::NotStarted) {
        if !input.as_ref().exists() {
            eprintln!("Input file does not exist");
            return CommandExitResult::ExitProgram;
        }

        if let Some(output) = &output {
            if output.as_ref().exists() && !overwrite {
                print!("Output file already exists. Overwrite? (y/n): ");
                io::stdout().flush().unwrap();

                let mut input = String::new();
                io::stdin().read_line(&mut input).unwrap();
                if input.trim().to_lowercase() != "y" {
                    return CommandExitResult::ExitProgram;
                }
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
        .replace(|c: char| !c.is_ascii_hexdigit() && c != '\n', "");

        let mut queue = VecDeque::new();

        fn push_sample(queue: &mut VecDeque<Vec<u8>>, sample: &str) -> CommandExitResult {
            let actual_bytes = (0..sample.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&sample[i..i + 2], 16).unwrap())
                .collect::<Vec<u8>>();

            if actual_bytes.len() > 100 {
                eprintln!("Input file too large");
                return CommandExitResult::ExitProgram;
            }

            queue.push_back(actual_bytes);
            CommandExitResult::Operational
        }

        if bulk {
            for sample in input_buf.split('\n') {
                let result = push_sample(&mut queue, sample);
                if !matches!(result, CommandExitResult::Operational) {
                    return result;
                }
            }
        } else {
            let sample = input_buf.replace('\n', "");
            let result = push_sample(&mut queue, &sample);
            if !matches!(result, CommandExitResult::Operational) {
                return result;
            }
        }

        *state = ManualExecutionState::Started {
            output: output.as_ref().map(|x| x.as_ref().to_path_buf()),
            samples: queue,
        };
    }

    let result = net_fuzzing_pretext(udp, db, &mut None, &mut None).await;
    if result != CommandExitResult::Operational {
        return result;
    }

    let state = match state {
        ManualExecutionState::NotStarted => {
            unreachable!()
        }
        ManualExecutionState::Started { output, samples } => (output, samples),
    };

    while let Some(sample) = state.1.front() {
        info!("Sample queue size: {}", state.1.len());
        let (result, events) = match net_execute_sample(udp, interface, db, &sample).await {
            ExecuteSampleResult::Timeout => return CommandExitResult::RetryOrReconnect,
            ExecuteSampleResult::Rerun => return CommandExitResult::Operational,
            ExecuteSampleResult::Success((a, b)) => (a, b),
        };

        db.push_results(sample.to_vec(), result.clone(), events.clone(), 0, 0);
        let _ = db.save().await.map_err(|e| {
            error!("Failed to save the database: {:?}", e);
        });
        let sample = state.1.pop_front().unwrap();

        if let Some(output) = state.0 {
            let mut output_text = "Disassembly:\n".to_string();
            disassemble_code(&sample, &mut output_text);

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
            let count_mem_override = events
                .iter()
                .filter(|e| matches!(e, fuzzer_data::ReportExecutionProblem::AccessCoverageArea))
                .count();
            let other = events.len()
                - count_very_like_bug
                - count_serialized_mismatch
                - count_trace_mismatch
                - count_coverage_mismatch
                - count_mem_override;

            println!("\nEvent summary:");
            println!("  - Very likely bug: {}", count_very_like_bug);
            println!("  - Serialized mismatch: {}", count_serialized_mismatch);
            println!("  - State trace mismatch: {}", count_trace_mismatch);
            println!("  - Coverage mismatch: {}", count_coverage_mismatch);
            println!("  - Memory override: {}", count_mem_override);
            println!("  - Other: {}", other);

            output_text.push_str("\n\nExit result:\n");
            output_text.push_str(&format!(
                "{}",
                serde_json::to_string_pretty(&result).unwrap()
            ));
            output_text.push_str("\nEND-EXIT\n");

            output_text.push_str("\nHREs:\n");
            let very_likely_bugs = events
                .iter()
                .filter(|e| matches!(e, fuzzer_data::ReportExecutionProblem::VeryLikelyBug { .. }));
            let serialized_mismatches = events.iter().filter(|e| {
                matches!(
                    e,
                    fuzzer_data::ReportExecutionProblem::SerializedMismatch { .. }
                )
            });
            let state_trace_mismatches = events.iter().filter(|e| {
                matches!(
                    e,
                    fuzzer_data::ReportExecutionProblem::StateTraceMismatch { .. }
                )
            });
            let coverage_mismatches = events.iter().filter(|e| {
                matches!(e, fuzzer_data::ReportExecutionProblem::CoverageProblem { .. })
            });
            for event in very_likely_bugs {
                output_text.push_str(&format!(
                    "- Very likely bug: {}\n",
                    serde_json::to_string_pretty(event).unwrap()
                ));
            }
            for event in serialized_mismatches {
                if let ReportExecutionProblem::SerializedMismatch {
                    serialized_exit,
                    serialized_state,
                } = event
                {
                    output_text.push_str("- Serialization Problem: \n");
                    if let Some(exit) = serialized_exit {
                        let difference = result.exit.difference(exit);
                        if difference.is_empty() {
                            output_text.push_str("   Exit: No differences found\n");
                        }
                        for diff in difference {
                            output_text.push_str(&format!(
                                "   Exit: {}: {:x?} -> {:x?}\n",
                                diff.0, diff.1, diff.2
                            ));
                        }
                    }
                    if let Some(state) = serialized_state {
                        let difference = result.state.difference(state);
                        if difference.is_empty() {
                            output_text.push_str("   State: No differences found\n");
                        }
                        for diff in difference {
                            output_text.push_str(&format!(
                                "   State: {}: {:x?} -> {:x?}\n",
                                diff.0, diff.1, diff.2
                            ));
                        }
                    }
                }
            }
            for event in state_trace_mismatches {
                if let ReportExecutionProblem::StateTraceMismatch {
                    index,
                    normal,
                    serialized,
                } = event
                {
                    let rip_normal = normal.as_ref().map(|x| x.standard_registers.rip).map(|x| format!("{:x}", x)).unwrap_or_default();
                    let rip_serialized = serialized.as_ref().map(|x| x.standard_registers.rip).map(|x| format!("{:x}", x)).unwrap_or_default();
                    output_text.push_str(&format!("- State Trace Mismatch [{index}] [{rip_normal}:{rip_serialized}]:\n"));
                    if let (Some(normal), Some(serialized)) = (normal, serialized) {
                        let difference = normal.difference(serialized);
                        if difference.is_empty() {
                            output_text.push_str("   No differences found\n");
                        } else {
                            for diff in difference {
                                output_text.push_str(&format!(
                                    "   {}: {:x?} -> {:x?}\n",
                                    diff.0, diff.1, diff.2
                                ));
                            }
                        }
                    } else {
                        output_text.push_str(&format!(
                            "   Trace divergence {}:{}\n",
                            normal.is_some(),
                            serialized.is_some()
                        ));
                    }
                }
            }
            for event in coverage_mismatches {
                if let ReportExecutionProblem::CoverageProblem { address, coverage_exit, coverage_state } = event {
                    output_text.push_str(&format!(
                        "- Coverage Problem at {:04x}:\n",
                        address
                    ));
                    if let Some(exit) = coverage_exit {
                        let difference = result.exit.difference(exit);
                        if difference.is_empty() {
                            output_text.push_str("   Exit: No differences found\n");
                        }
                        for diff in difference {
                            output_text.push_str(&format!(
                                "   Exit: {}: {:x?} -> {:x?}\n",
                                diff.0, diff.1, diff.2
                            ));
                        }
                    }
                    if let Some(state) = coverage_state {
                        let difference = result.state.difference(state);
                        if difference.is_empty() {
                            output_text.push_str("   State: No differences found\n");
                        }
                        for diff in difference {
                            output_text.push_str(&format!(
                                "   State: {}: {:x?} -> {:x?}\n",
                                diff.0, diff.1, diff.2
                            ));
                        }
                    }
                }
            }

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

            if let Err(e) = std::fs::write(output, output_text) {
                eprintln!("Failed to write output file: {}", e);
                return CommandExitResult::ExitProgram;
            }
        }
    }

    CommandExitResult::ExitProgram
}

/// Disassembles code and formats it for analysis
///
/// This function takes a code sample and generates a formatted disassembly
/// with instruction addresses, bytes, and mnemonics.
///
/// # Arguments
///
/// * `code` - Code sample to disassemble
/// * `out` - String buffer to write disassembly to
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
