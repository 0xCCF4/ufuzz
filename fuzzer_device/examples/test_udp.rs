#![no_main]
#![no_std]

extern crate alloc;

use alloc::string::String;
use log::info;
use uefi::{entry, Status};
use uefi_raw::Ipv4Address;
use uefi_udp4::uefi_raw::protocol::network::udp4::Udp4ConfigData;
use uefi_udp4::Ipv4AddressExt;

#[entry]
unsafe fn main() -> Status {
    uefi::helpers::init().unwrap();

    let network_handles = match uefi::boot::find_handles::<
        uefi_udp4::uefi_raw::protocol::network::udp4::UDP4ServiceBindingProtocol,
    >() {
        Ok(handles) => handles,
        Err(e) => {
            info!("Failed to find network binding protocol handles: {:?}", e);
            return Status::ABORTED;
        }
    };

    for (i, handle) in network_handles.iter().enumerate() {
        info!("Found network binding protocol handle {}: {:?}", i, handle);
        let net = match uefi::boot::open_protocol_exclusive::<
            uefi_udp4::uefi_raw::protocol::network::udp4::UDP4ServiceBindingProtocol,
        >(*handle)
        {
            Ok(net) => net,
            Err(e) => {
                info!("Failed to open network binding protocol: {:?}", e);
                continue;
            }
        };

        let udp_handle = match net.create_child() {
            Ok(handle) => handle,
            Err(e) => {
                info!("Failed to create child handle: {:?}", e);
                continue;
            }
        };

        let mut net = match uefi::boot::open_protocol_exclusive::<
            uefi_udp4::uefi::proto::network::udp4::proto::UDP4Protocol,
        >(udp_handle.handle())
        {
            Ok(net) => net,
            Err(e) => {
                info!("Failed to open network protocol: {:?}", e);
                continue;
            }
        };

        if let Err(err) = net.reset() {
            info!("Failed to reset network protocol: {:?}", err);
            continue;
        }

        let remote_address = Ipv4Address::new(192, 168, 0, 4);
        let source_address = Ipv4Address::new(192, 168, 0, 44);
        let subnet_mask = Ipv4Address::new(255, 255, 255, 0);

        let remote_port = 4444;
        let source_port = 4444;

        let config_data = Udp4ConfigData {
            accept_any_port: false,
            accept_broadcast: true,
            accept_promiscuous: true,
            allow_duplicate_port: true,
            do_not_fragment: true,
            receive_timeout: 0,
            remote_address: remote_address.clone(),
            remote_port,
            station_port: source_port,
            time_to_live: 200,
            type_of_service: 0,
            transmit_timeout: 0,
            // use DHCP
            use_default_address: false,
            subnet_mask: subnet_mask.clone(),
            station_address: source_address.clone(),
        };

        if let Err(err) = net.configure(&config_data) {
            info!("Failed to configure network protocol: {:?}", err);
            continue;
        }

        let data = "Hello World from UEFI".as_bytes();

        let result = net.transmit(None, None, data);
        info!("Transmit result: {:?}", result);
        if let Err(err) = result {
            info!("Failed to transmit data: {:?}", err);
            continue;
        }

        let result = net.receive(None);
        info!("Receive result: {:x?}", result);
        info!("Lossy string: {:?}", result.as_ref().map(|v| String::from_utf8_lossy(v)));
    }

    Status::SUCCESS
}
