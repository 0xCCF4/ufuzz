use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use core::pin::Pin;
use fuzzer_data::{
    Ota, OtaC2D, OtaC2DUnreliable, OtaD2C, OtaD2CTransport, OtaD2CUnreliable, OtaPacket,
    MAX_FRAGMENT_SIZE, MAX_PAYLOAD_SIZE,
};
use log::{error, info, trace, warn};
use uefi::boot::ScopedProtocol;
use uefi::println;
use uefi_raw::Ipv4Address;
use uefi_udp4::uefi::proto::network::udp4::proto::{
    ReceiveError, TransmitError, UDP4Protocol, UdpChannel,
};
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

    TransmitLengthExceeded,
    TransmitPacket(TransmitError),
    TransmitWaitForAckFailedHard(Box<ConnectionError>),
    TransmitNotAcknowledged,

    ReceivePacket(ReceiveError),
    ReceiveTimeout,
    ReceiveNotUtf8,
    ReceiveNotDeserializable,
    PacketTooLarge,
    FragmentOutOfOrder,
    FragmentSessionClosed,
}

pub struct ConnectionSettings {
    pub remote_address: Ipv4Address,
    pub source_address: Ipv4Address,
    pub subnet_mask: Ipv4Address,
    pub remote_port: u16,
    pub source_port: u16,
    pub resent_attempts: u8,
    pub ack_timeout: u64,      // ms
    pub fragment_timeout: u64, // ms
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
            fragment_timeout: 1000,
        }
    }
}

pub struct ControllerConnection {
    udp_handle: Option<ScopedBindingProtocol<UDP4ServiceBindingProtocol>>,
    network: Option<Pin<Box<ScopedProtocol<UDP4Protocol>>>>,
    channel: Option<UdpChannel<'static>>,

    virtual_receive_buffer: VecDeque<OtaC2D>,
    resent_attempts: u8,
    ack_timeout: u64,
    fragment_timeout: u64,

    remote_session: u16,
    sequence_number_rx: u64,
    sequence_number_tx: u64,
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
            error!("Failed to configure network protocol: {:x?}", err);
            return Err(ConnectionError::FailedToConfigureConnection);
        }

        if let Err(err) = net.cancel_all() {
            error!("Failed to cancel all: {:x?}", err);
            return Err(ConnectionError::FailedToConfigureConnection);
        }

        match net.get_mode_data() {
            Ok(data) => {
                for route in data.ip4_mode_data.ip4_route_table.to_vec() {
                    if let Err(err) = net.delete_route(
                        &route.subnet_addr,
                        &route.subnet_mask,
                        &route.gateway_addr,
                    ) {
                        error!("Failed to delete route: {:?}", err);
                    }
                }
                let subnet = settings.remote_address.0;
                let subnet = (subnet[0] as u32) << 24
                    | (subnet[1] as u32) << 16
                    | (subnet[2] as u32) << 8
                    | subnet[3] as u32;
                let subnet_mask = settings.subnet_mask;
                let subnet_mask = (subnet_mask.0[0] as u32) << 24
                    | (subnet_mask.0[1] as u32) << 16
                    | (subnet_mask.0[2] as u32) << 8
                    | subnet_mask.0[3] as u32;

                let subnet_ip = subnet & subnet_mask;
                let subnet_ip = Ipv4Address::new(
                    (subnet_ip >> 24) as u8,
                    (subnet_ip >> 16) as u8,
                    (subnet_ip >> 8) as u8,
                    subnet_ip as u8,
                );
                let gateway_ip =
                    Ipv4Address::new(subnet_ip.0[0], subnet_ip.0[1], subnet_ip.0[2], 1);

                if let Err(err) = net.add_route(&subnet_ip, &settings.subnet_mask, &gateway_ip) {
                    error!("Failed to add route: {:?}", err);
                }
            }
            Err(err) => {
                info!("Failed to get mode data: {:?}", err);
            }
        }

        let mut net = Box::pin(net);
        let net_ref: &mut ScopedProtocol<UDP4Protocol> = &mut net;
        let net_ref: &'static mut ScopedProtocol<UDP4Protocol> =
            unsafe { core::mem::transmute(net_ref) }; // safe, since from now on, net will not be used anymore
        let channel: UdpChannel<'static> = net_ref.channel();

        Ok(Self {
            udp_handle: Some(udp_handle),
            channel: Some(channel),
            network: Some(net),
            ack_timeout: settings.ack_timeout,
            fragment_timeout: settings.fragment_timeout,
            resent_attempts: settings.resent_attempts,
            virtual_receive_buffer: VecDeque::new(),

            remote_session: 0,
            sequence_number_rx: 0,
            sequence_number_tx: 0,
        })
    }

    pub fn send<Packet: OtaPacket<OtaD2CUnreliable, OtaD2CTransport>>(
        &mut self,
        data: Packet,
    ) -> Result<(), ConnectionError> {
        let mut packet = if data.reliable_transport() {
            self.sequence_number_tx += 1;

            data.to_packet(self.sequence_number_tx, self.remote_session)
        } else {
            data.to_packet(0, 0)
        };

        let buf = packet.serialize().expect("Must always serialize");

        if buf.len() as u64 > MAX_FRAGMENT_SIZE {
            // fragment

            return Err(ConnectionError::TransmitLengthExceeded); // todo

            if let Ota::Transport { id, session, .. } = &mut packet {
                *id = 0;
                *session = 0;
            }

            drop(buf);
            let buf = packet.serialize().expect("Must always serialize");

            let chunks = buf
                .chunks(MAX_FRAGMENT_SIZE as usize - 128)
                .collect::<Vec<&[u8]>>();
            for (i, chunk) in chunks.iter().enumerate() {
                self.sequence_number_tx += 1;
                let packet = OtaD2C::ChunkedTransport {
                    session: self.remote_session,
                    id: self.sequence_number_tx,
                    fragment: i as u64,
                    total_fragments: chunks.len() as u64,
                    content: chunk.to_vec(),
                };
                let buf = packet.serialize().expect("Must always serialize");
                self.send_native(&buf, true)?;
            }
            Ok(())
        } else {
            // just send
            self.send_native(&buf, matches!(packet, Ota::Transport { .. }))
        }
    }

    fn send_native(&mut self, data: &[u8], require_ack: bool) -> Result<(), ConnectionError> {
        if data.len() as u64 > MAX_FRAGMENT_SIZE {
            return Err(ConnectionError::TransmitLengthExceeded);
        }

        let mut virtual_receive_buffer = VecDeque::new();

        let mut status = None;
        'attempt_loop: for _attempt in 0..self.resent_attempts {
            // initial packet sending
            if let Err(err) = self.channel.as_mut().unwrap().transmit(None, None, data) {
                error!("Failed to transmit data: {:?}", err);
                status = Some(Err(ConnectionError::TransmitPacket(err)));
                break 'attempt_loop;
            }

            // check if requires ack
            if !require_ack {
                // does not require ack
                status = Some(Ok(()));
                break 'attempt_loop;
            }

            // wait for ack
            'wait_for_ack: while let Some(received_packet) = {
                match self.receive(Some(self.ack_timeout)) {
                    Ok(None) => continue 'wait_for_ack,
                    Ok(Some(packet)) => Some(packet),
                    Err(err) => {
                        if !matches!(err, ConnectionError::ReceiveTimeout) {
                            error!("Failed to transmit data: {:?}", err);
                            status = Some(Err(ConnectionError::TransmitWaitForAckFailedHard(
                                Box::new(err),
                            )));
                            break 'attempt_loop;
                        }
                        continue 'attempt_loop;
                    }
                }
            } {
                if let OtaC2D::Unreliable(OtaC2DUnreliable::Ack(sequence_number)) = received_packet
                {
                    if sequence_number > self.sequence_number_tx {
                        warn!(
                            "Received ack for future sequence number: {}",
                            sequence_number
                        );
                        virtual_receive_buffer.push_back(received_packet);
                    } else if sequence_number == self.sequence_number_tx {
                        // OK received acknowledgement
                        status = Some(Ok(()));
                        break 'attempt_loop;
                    } else {
                        #[cfg(feature = "__debug_print_udp")]
                        info!("Received ack for past sequence number: {}", sequence_number);
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

        status.unwrap_or(Err(ConnectionError::TransmitNotAcknowledged))
    }

    fn receive_native(
        &mut self,
        timeout_millis: Option<u64>,
    ) -> Result<Option<OtaC2D>, ConnectionError> {
        if let Some(packet) = self.virtual_receive_buffer.pop_front() {
            return Ok(Some(packet));
        }

        let data = match self.channel.as_mut().unwrap().receive(timeout_millis) {
            Ok(data) => data,
            Err(e) => {
                if matches!(e, ReceiveError::ReceiveTimeout) {
                    return Err(ConnectionError::ReceiveTimeout);
                }
                error!("Failed to receive data: {:x?}", e);
                return Err(ConnectionError::ReceivePacket(e));
            }
        };

        let data: OtaC2D = match OtaC2D::deserialize(&data) {
            Ok(data) => data,
            Err(e) => {
                error!("Failed to deserialize data: {:?}", e);
                return Err(ConnectionError::ReceiveNotDeserializable);
            }
        };

        if let Some(ack) = data.ack() {
            if let Err(err) = self.send(ack) {
                error!("Failed to send ack: {:?}", err);
                return Err(err);
            }
        }

        #[cfg(feature = "__debug_print_udp")]
        if matches!(data, Ota::Transport { .. }) {
            trace!(
                "Pre-Received: {:?}, cur seq {}",
                data,
                self.sequence_number_rx
            );
        }

        if let Ota::Transport { session, id, .. } | Ota::ChunkedTransport { session, id, .. } =
            &data
        {
            if *session != self.remote_session {
                #[cfg(feature = "__debug_print_udp")]
                warn!("Received packet with new session: {}", session);
                self.sequence_number_rx = *id;
                self.remote_session = *session;
            } else if *id < self.sequence_number_rx {
                #[cfg(feature = "__debug_print_udp")]
                warn!("Received packet with retired sequence number: {}", id);
                return Ok(None);
            } else if *id == self.sequence_number_rx {
                #[cfg(feature = "__debug_print_udp")]
                warn!("Received packet with same sequence number: {}", id);
                return Ok(None);
            } else {
                // *id > self.sequence_number_rx
                self.sequence_number_rx = *id;
            }
        }

        #[cfg(feature = "__debug_print_udp")]
        if matches!(data, Ota::Transport { .. }) {
            trace!("Received: {:?}, cur seq {}", data, self.sequence_number_rx);
        }

        Ok(Some(data))
    }

    pub fn receive(
        &mut self,
        timeout_millis: Option<u64>,
    ) -> Result<Option<OtaC2D>, ConnectionError> {
        let initial_packet = match self.receive_native(timeout_millis)? {
            Some(packet) => packet,
            None => return Ok(None),
        };

        match initial_packet {
            OtaC2D::Transport { .. } | OtaC2D::Unreliable(_) => Ok(Some(initial_packet)),
            OtaC2D::ChunkedTransport {
                session,
                id,
                content,
                fragment,
                total_fragments,
            } => {
                return Err(ConnectionError::TransmitLengthExceeded); // todo

                if total_fragments.saturating_mul(MAX_FRAGMENT_SIZE as u64)
                    > MAX_PAYLOAD_SIZE as u64
                {
                    return Err(ConnectionError::PacketTooLarge);
                }

                if fragment != 0 {
                    return Err(ConnectionError::FragmentOutOfOrder);
                }

                let mut packet_content: Vec<u8> = content;

                for i in 1..total_fragments {
                    'wait_for_next_fragment: while let Some(received_packet) = {
                        match self.receive_native(Some(self.fragment_timeout)) {
                            Ok(None) => continue 'wait_for_next_fragment,
                            Ok(Some(packet)) => Some(packet),
                            Err(err) => {
                                return Err(err);
                            }
                        }
                    } {
                        if let OtaC2D::ChunkedTransport {
                            session: received_session,
                            id: received_id,
                            content: received_content,
                            fragment: received_fragment,
                            total_fragments: received_total_fragments,
                        } = received_packet
                        {
                            if session != received_session {
                                error!("Received packet with new session: {}", received_session);
                                return Err(ConnectionError::FragmentSessionClosed);
                            }
                            if i != received_fragment {
                                return Err(ConnectionError::FragmentOutOfOrder);
                            }
                            packet_content.extend(received_content);
                        } else {
                            // received other package
                            trace!("Dropped packet: {:?}", received_packet);
                        }
                    }
                }

                let data: OtaC2D = match OtaC2D::deserialize(&packet_content) {
                    Ok(data) => data,
                    Err(e) => {
                        error!("Failed to deserialize data: {:?}", e);
                        return Err(ConnectionError::ReceiveNotDeserializable);
                    }
                };
                Ok(Some(data))
            }
        }
    }
}

impl Drop for ControllerConnection {
    fn drop(&mut self) {
        drop(self.channel.take());
        drop(self.network.take());
        drop(self.udp_handle.take());
    }
}
