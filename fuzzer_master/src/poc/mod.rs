use crate::device_connection::DeviceConnection;
use crate::fuzzer_node_bridge::FuzzerNodeInterface;
use crate::CommandExitResult;
use clap::Parser;
use fuzzer_data::{Ota, OtaC2DTransport, OtaD2CTransport};
use poc_data::{f0_microcode, f1_microspectre};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::time::Duration;

#[derive(Parser, Debug, Clone)]
pub enum Cli {
    /// F0: POC that microcode update works
    ///
    /// Available commands: reset, enable, disable, random
    ///
    /// Expected usage: 1. call reset to reset microcode update,
    /// then call in any order random, enable, disable
    ///
    /// Expected observation: If microcode update for RDRAND x86 instruction
    /// is disabled then random should produce random values
    /// When enabled, the same 'random' value is return upon random invocation
    #[command(subcommand)]
    MicrocodeUpdates(f0_microcode::Payload),

    #[command(subcommand)]
    MicroSpectre(f1_microspectre::Payload),
}

pub async fn main(
    data: &Cli,
    net: &mut DeviceConnection,
    _interface: &FuzzerNodeInterface,
) -> Result<CommandExitResult, Box<dyn Error>> {
    match data {
        Cli::MicrocodeUpdates(payload) => {
            println!("[POC] Running poc #F0, proofing that microcode updates work");

            let result = run_scenario::<&f0_microcode::Payload, f0_microcode::ResultingData>(
                net,
                f0_microcode::NAME,
                payload,
            )
            .await?;

            match result {
                f0_microcode::ResultingData::Ok => {
                    println!("[POC] command {payload:?} was successful");
                }
                f0_microcode::ResultingData::Error(err) => {
                    println!("[POC] fuzzing agent returned ERR:{err}");
                }
                f0_microcode::ResultingData::RandomNumbers(vec) => {
                    println!("[POC] fuzzing agent returned random numbers");
                    for number in vec {
                        print!("{number:>3?} ");
                    }
                    println!();
                }
            }

            Ok(CommandExitResult::ExitProgram)
        }

        Cli::MicroSpectre(payload) => {
            let result = run_scenario::<&f1_microcode::Payload, f1_microcode::ResultingData>(
                net,
                f1_microcode::NAME,
                payload,
            )
            .await?;
        }
    }
}

async fn run_scenario<T: Serialize, Q: for<'a> Deserialize<'a>>(
    net: &mut DeviceConnection,
    name: &str,
    data: T,
) -> Result<Q, String> {
    if let Err(err) = net
        .send(OtaC2DTransport::RunScenario(
            name.to_string(),
            poc_data::serialize(&data)?,
        ))
        .await
    {
        return Err(format!(
            "Failed to instruct agent to run scenario: {}. Is the agent running or busy?",
            err
        ));
    }

    loop {
        let packet = net.receive(Some(Duration::from_secs(30))).await;
        if let Some(packet) = packet {
            if let Ota::Transport { content, .. } = packet {
                if let OtaD2CTransport::ScenarioResult(recv_name, data) = content {
                    if recv_name != name {
                        return Err(format!("Received unexpected scenario result for scenario {recv_name} but we are running {name}."));
                    }
                    return Ok(poc_data::deserialize(&data)?);
                }
            }
            continue;
        }
        // timeout
        return Err("Timeout while waiting for scenario response".to_string());
    }
}
