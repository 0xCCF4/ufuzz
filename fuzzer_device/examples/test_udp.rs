#![no_main]
#![no_std]

extern crate alloc;

use alloc::format;
use alloc::vec::Vec;
use fuzzer_data::{OtaD2CTransport, MAX_FRAGMENT_SIZE};
use fuzzer_device::controller_connection::{ConnectionSettings, ControllerConnection};
use log::{info, Level};
use uefi::{entry, Status};

#[entry]
unsafe fn main() -> Status {
    uefi::helpers::init().unwrap();

    let mut udp = match ControllerConnection::connect(&ConnectionSettings::default()) {
        Ok(udp) => udp,
        Err(e) => {
            info!("Failed to connect to controller: {:?}", e);
            return Status::ABORTED;
        }
    };

    let mut numbers: Vec<u8> = Vec::new();
    for x in 0..MAX_FRAGMENT_SIZE as u16 {
        numbers.push((x & 0xff) as u8);
    }

    while let Ok(x) = udp.receive(Some(60_000)) {
        if let Some(x) = x {
            info!("Received: {:?}", x);
            if let Err(err) = udp.send(OtaD2CTransport::LogMessage {
                level: Level::Info,
                message: format!("Echo: {:?}", x),
            }) {
                info!("Failed to send: {:?}", err);
                return Status::ABORTED;
            }
        }
    }

    Status::SUCCESS
}
