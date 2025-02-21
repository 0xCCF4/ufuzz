use alloc::collections::VecDeque;
use alloc::string::String;
use fuzzer_data::{OTAControllerToDevice, OTADeviceToController};
use log::{error, info, warn};
use uefi::boot::ScopedProtocol;
use uefi_raw::Ipv4Address;
use uefi_udp4::uefi::proto::network::udp4::proto::UDP4Protocol;
use uefi_udp4::uefi_raw::protocol::network::udp4::{
    ScopedBindingProtocol, UDP4ServiceBindingProtocol, Udp4ConfigData,
};
use uefi_udp4::Ipv4AddressExt;

#[derive(Debug)]
pub enum ConnectionError {
    NoNetworkProtocol,
    CouldNotOpenNetProto,
    FailedToCreateConnection,
    FailedToOpenChildConnection,
    FailedToResetConnection,
    FailedToConfigureConnection,
    FailedToTransmit,
    FailedToReceive,
    AckNotReceived,
}

pub struct ConnectionSettings {
    pub remote_address: Ipv4Address,
    pub source_address: Ipv4Address,
    pub subnet_mask: Ipv4Address,
    pub remote_port: u16,
    pub source_port: u16,
    pub resent_attempts: u8,
    pub ack_timeout: u64, // ms
}

impl Default for ConnectionSettings {
    fn default() -> Self {
        Self {
            remote_address: Ipv4Address::new(192, 168, 0, 4),
            source_address: Ipv4Address::new(192, 168, 0, 44),
            subnet_mask: Ipv4Address::new(255, 255, 255, 0),
            remote_port: 4444,
            source_port: 4444,
            resent_attempts: 10,
            ack_timeout: 200,
        }
    }
}

pub struct ControllerConnection {
    udp_handle: Option<ScopedBindingProtocol<UDP4ServiceBindingProtocol>>,
    network: Option<ScopedProtocol<UDP4Protocol>>,
    virtual_receive_buffer: VecDeque<OTAControllerToDevice>,
    resent_attempts: u8,
    ack_timeout: u64,
}

impl ControllerConnection {
    pub fn connect(settings: &ConnectionSettings) -> Result<Self, ConnectionError> {
        let binding_handle = match uefi::boot::find_handles::<UDP4ServiceBindingProtocol>() {
            Ok(handles) => handles,
            Err(e) => {
                error!("Failed to find network binding protocol handles: {:?}", e);
                return Err(ConnectionError::NoNetworkProtocol);
            }
        };

        let binding_handle = {
            if binding_handle.len() == 0 {
                error!("No network binding protocol handles found");
                return Err(ConnectionError::NoNetworkProtocol);
            } else if binding_handle.len() > 1 {
                warn!("Multiple network binding protocol handles found, using the first one");
            }
            binding_handle[0]
        };

        let binding_proto =
            match uefi::boot::open_protocol_exclusive::<UDP4ServiceBindingProtocol>(binding_handle)
            {
                Ok(handles) => handles,
                Err(e) => {
                    error!("Failed to find network binding protocol handles: {:?}", e);
                    return Err(ConnectionError::CouldNotOpenNetProto);
                }
            };

        let udp_handle = match binding_proto.create_child() {
            Ok(handle) => handle,
            Err(e) => {
                error!("Failed to create child handle: {:?}", e);
                return Err(ConnectionError::FailedToCreateConnection);
            }
        };

        drop(binding_proto);

        let mut net = match uefi::boot::open_protocol_exclusive::<UDP4Protocol>(udp_handle.handle())
        {
            Ok(net) => net,
            Err(e) => {
                info!("Failed to open network protocol: {:?}", e);
                return Err(ConnectionError::CouldNotOpenNetProto);
            }
        };

        if let Err(err) = net.reset() {
            info!("Failed to reset network protocol: {:?}", err);
            return Err(ConnectionError::FailedToResetConnection);
        }

        // connect using: nc -p 4444 -u 192.168.0.44 4444

        let config_data = Udp4ConfigData {
            accept_any_port: true,
            accept_broadcast: false,
            accept_promiscuous: false,
            allow_duplicate_port: true,
            do_not_fragment: true,
            receive_timeout: 0,
            remote_address: settings.remote_address.clone(),
            remote_port: settings.remote_port,
            station_port: settings.source_port,
            time_to_live: 200,
            type_of_service: 0,
            transmit_timeout: 0,
            // use DHCP
            use_default_address: false,
            subnet_mask: settings.subnet_mask.clone(),
            station_address: settings.source_address.clone(),
        };

        if let Err(err) = net.configure(&config_data) {
            error!("Failed to configure network protocol: {:?}", err);
            return Err(ConnectionError::FailedToConfigureConnection);
        }

        Ok(Self {
            udp_handle: Some(udp_handle),
            network: Some(net),
            ack_timeout: settings.ack_timeout,
            resent_attempts: settings.resent_attempts,
            virtual_receive_buffer: VecDeque::new(),
        })
    }

    pub fn send(&mut self, data: &OTADeviceToController) -> Result<(), ConnectionError> {
        let string = serde_json::to_string(data).expect("Must always serialize");
        let buf = string.as_bytes();

        if buf.len() > 4000 {
            return Err(ConnectionError::FailedToTransmit);
        }

        let mut virtual_receive_buffer = VecDeque::new();

        let mut status = None;
        'attempt_loop: for _attempt in 0..self.resent_attempts {
            // initial packet sending
            if let Err(err) = self.network.as_mut().unwrap().transmit(None, None, buf) {
                error!("Failed to transmit data: {:?}", err);
                status = Some(Err(ConnectionError::FailedToTransmit));
                break 'attempt_loop;
            }

            // check if requires ack
            if !data.requires_ack() {
                // does not require ack
                status = Some(Ok(()));
                break 'attempt_loop;
            }

            // wait for ack
            while let Some(received_packet) = match self.receive(Some(self.ack_timeout)) {
                Ok(packet) => Some(packet),
                Err(err) => {
                    error!("Failed to transmit data: {:?}", err);
                    status = Some(Err(ConnectionError::FailedToTransmit));
                    break 'attempt_loop;
                }
            } {
                if let OTAControllerToDevice::Ack(ack_content) = received_packet {
                    if data == ack_content.as_ref() {
                        // OK received acknowledgement
                        status = Some(Ok(()));
                        break 'attempt_loop;
                    }
                } else {
                    // received other package
                    virtual_receive_buffer.push_back(received_packet);
                }
            }
        }

        // requeue the received packets
        for packet in virtual_receive_buffer.into_iter().rev() {
            self.virtual_receive_buffer.push_front(packet);
        }

        status.unwrap_or(Err(ConnectionError::AckNotReceived))
    }

    pub fn receive(
        &mut self,
        timeout_millis: Option<u64>,
    ) -> Result<OTAControllerToDevice, ConnectionError> {
        if let Some(packet) = self.virtual_receive_buffer.pop_front() {
            return Ok(packet);
        }

        let data = match self.network.as_mut().unwrap().receive(timeout_millis) {
            Ok(data) => data,
            Err(e) => {
                error!("Failed to receive data: {:?}", e);
                return Err(ConnectionError::FailedToReceive);
            }
        };

        let string = match String::from_utf8(data) {
            Ok(string) => string,
            Err(e) => {
                error!("Failed to convert data to string: {:?}", e);
                return Err(ConnectionError::FailedToReceive);
            }
        };

        let data: OTAControllerToDevice = match serde_json::from_str(&string) {
            Ok(data) => data,
            Err(e) => {
                error!("Failed to deserialize data: {:?}", e);
                return Err(ConnectionError::FailedToReceive);
            }
        };

        if let Some(ack) = data.ack() {
            if let Err(err) = self.send(&ack) {
                error!("Failed to send ack: {:?}", err);
                return Err(err);
            }
        }

        Ok(data)
    }
}

impl Drop for ControllerConnection {
    fn drop(&mut self) {
        drop(self.network.take());
        drop(self.udp_handle.take());
    }
}
