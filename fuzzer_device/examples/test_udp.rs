#![no_main]
#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use fuzzer_data::{OtaD2CTransport, MAX_FRAGMENT_SIZE};
use fuzzer_device::controller_connection::{ConnectionSettings, ControllerConnection};
use log::info;
use uefi::{entry, println, Status};
use uefi_raw::Ipv4Address;
use uefi_udp4::uefi_raw::protocol::network::udp4::Udp4ConfigData;
use uefi_udp4::Ipv4AddressExt;

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
            if let Err(err) = udp.send_native(&numbers, false) {
                info!("Failed to send: {:?}", err);
                return Status::ABORTED;
            }
        }
    }

    Status::SUCCESS
}
