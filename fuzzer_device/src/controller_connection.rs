use log::{error, info, warn};
use uefi::boot::ScopedProtocol;
use uefi_raw::Ipv4Address;
use uefi_udp4::uefi::proto::network::udp4::proto::UDP4Protocol;
use uefi_udp4::uefi_raw::protocol::network::udp4::{
    ScopedBindingProtocol, UDP4ServiceBindingProtocol, Udp4ConfigData,
};
use uefi_udp4::Ipv4AddressExt;

pub enum ConnectionError {
    NoNetworkProtocol,
    CouldNotOpenNetProto,
    FailedToCreateConnection,
    FailedToOpenChildConnection,
    FailedToResetConnection,
    FailedToConfigureConnection,
    FailedToTransmit,
    FailedToReceive,
}

pub struct ConnectionSettings {
    pub remote_address: Ipv4Address,
    pub source_address: Ipv4Address,
    pub subnet_mask: Ipv4Address,
    pub remote_port: u16,
    pub source_port: u16,
}

impl Default for ConnectionSettings {
    fn default() -> Self {
        Self {
            remote_address: Ipv4Address::new(192, 168, 0, 4),
            source_address: Ipv4Address::new(192, 168, 0, 44),
            subnet_mask: Ipv4Address::new(255, 255, 255, 0),
            remote_port: 4444,
            source_port: 4444,
        }
    }
}

pub struct ControllerConnection {
    udp_handle: ScopedBindingProtocol<UDP4ServiceBindingProtocol>,
    network: ScopedProtocol<UDP4Protocol>,
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
            udp_handle,
            network: net,
        })
    }
}
