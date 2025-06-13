//! Controller Connection Module
//!
//! This module provides functionality for establishing and managing network connections
//! between the fuzzer device and the fuzzing controller. It implements a reliable
//! UDP-based communication protocol.
//!
//! The module provides a high-level interface for sending and receiving messages
//! between the fuzzer device and controller, handling all the low-level details
//! of network communication and protocol management.

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::format;
use alloc::vec::Vec;
use core::fmt::Display;
use core::pin::Pin;
use fuzzer_data::{
    Ota, OtaC2D, OtaC2DUnreliable, OtaD2C, OtaD2CTransport, OtaD2CUnreliable, OtaPacket,
    MAX_FRAGMENT_SIZE, MAX_PAYLOAD_SIZE,
};
use log::{error, info, trace, warn};
#[cfg(feature = "__debug_performance_trace")]
use performance_timing::track_time;
use uefi::boot::ScopedProtocol;
use uefi_raw::Ipv4Address;
use uefi_udp4::uefi::proto::network::udp4::proto::{
    ReceiveError, TransmitError, UDP4Protocol, UdpChannel,
};
use uefi_udp4::uefi_raw::protocol::network::udp4::{
    ScopedBindingProtocol, UDP4ServiceBindingProtocol, Udp4ConfigData,
};
use uefi_udp4::Ipv4AddressExt;

/// Errors that can occur during controller connection operations
#[derive(Debug)]
pub enum ConnectionError {
    /// No network protocol was found
    NoNetworkProtocol,
    /// Failed to open the network protocol
    CouldNotOpenNetProto,
    /// Failed to create a new connection
    FailedToCreateConnection,
    /// Failed to open a child connection
    FailedToOpenChildConnection,
    /// Failed to reset the connection
    FailedToResetConnection,
    /// Failed to configure the connection
    FailedToConfigureConnection,

    /// Transmitted data exceeded maximum length
    TransmitLengthExceeded,
    /// Error during packet transmission
    TransmitPacket(TransmitError),
    /// Failed to wait for acknowledgment after multiple retries
    TransmitWaitForAckFailedHard(Box<ConnectionError>),
    /// Packet was not acknowledged
    TransmitNotAcknowledged,

    /// Error during packet reception
    ReceivePacket(ReceiveError),
    /// Reception timed out
    ReceiveTimeout,
    /// Received data is not valid UTF-8
    ReceiveNotUtf8,
    /// Received data could not be deserialized
    ReceiveNotDeserializable,
    /// Received packet is too large
    PacketTooLarge,
    /// Received fragment is out of order
    FragmentOutOfOrder,
    /// Fragment session was closed
    FragmentSessionClosed,
}

/// Configuration settings for the controller connection
pub struct ConnectionSettings {
    /// IP address of the remote controller
    pub remote_address: Ipv4Address,
    /// IP address of the local device
    pub source_address: Ipv4Address,
    /// Network subnet mask
    pub subnet_mask: Ipv4Address,
    /// UDP port of the remote controller
    pub remote_port: u16,
    /// UDP port of the local device
    pub source_port: u16,
    /// Number of retransmission attempts
    pub resent_attempts: u8,
    /// Timeout for acknowledgment in milliseconds
    pub ack_timeout: u64,
    /// Timeout for fragment reception in milliseconds
    pub fragment_timeout: u64,
}

impl Default for ConnectionSettings {
    /// Creates default connection settings
    ///
    /// Must be changed when using different experiment setups
    fn default() -> Self {
        Self {
            remote_address: Ipv4Address::new(10, 83, 3, 250),
            source_address: Ipv4Address::new(10, 83, 3, 6),
            subnet_mask: Ipv4Address::new(255, 255, 255, 0),
            remote_port: 4444,
            source_port: 4444,
            resent_attempts: 10,
            ack_timeout: 200,
            fragment_timeout: 1000,
        }
    }
}

/// Manages the connection to the fuzzing controller
///
/// This structure handles all aspects of communication with the controller,
/// including connection management, message sending and receiving, and
/// protocol state tracking.
pub struct ControllerConnection {
    /// UDP service binding protocol handle
    udp_handle: Option<ScopedBindingProtocol<UDP4ServiceBindingProtocol>>,
    /// Network protocol instance
    network: Option<Pin<Box<ScopedProtocol<UDP4Protocol>>>>,
    /// UDP channel for communication
    channel: Option<UdpChannel<'static>>,

    /// Buffer for received messages
    virtual_receive_buffer: VecDeque<OtaC2D>,
    /// Number of retransmission attempts
    resent_attempts: u8,
    /// Timeout for acknowledgment in milliseconds
    ack_timeout: u64,
    /// Timeout for fragment reception in milliseconds
    fragment_timeout: u64,

    /// Current remote session identifier
    remote_session: u16,
    /// Next sequence number for received messages
    sequence_number_rx: u64,
    /// Next sequence number for transmitted messages
    sequence_number_tx: u64,
}

#[cfg_attr(feature = "__debug_performance_trace", track_time)]
impl ControllerConnection {
    /// Establishes a connection to the controller
    ///
    /// # Arguments
    ///
    /// * `settings` - Connection configuration settings
    ///
    /// # Returns
    ///
    /// * `Ok(Self)` - A new controller connection instance
    /// * `Err(ConnectionError)` - An error occurred during connection
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

    /// Sends a packet to the controller
    ///
    /// # Arguments
    ///
    /// * `data` - The packet to send
    ///
    /// # Returns
    ///
    /// * `Ok(())` - Packet was sent successfully
    /// * `Err(ConnectionError)` - An error occurred during transmission
    #[allow(unreachable_code)]
    pub fn send<Packet: OtaPacket<OtaD2CUnreliable, OtaD2CTransport>>(
        &mut self,
        data: Packet,
    ) -> Result<(), ConnectionError> {
        let packet = if data.reliable_transport() {
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

    /// Sends raw data to the controller
    ///
    /// # Arguments
    ///
    /// * `data` - The data to send
    /// * `require_ack` - Whether to wait for acknowledgment
    ///
    /// # Returns
    ///
    /// * `Ok(())` - Data was sent successfully
    /// * `Err(ConnectionError)` - An error occurred during transmission
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

    /// Receives raw data from the controller
    ///
    /// # Arguments
    ///
    /// * `timeout_millis` - Optional timeout in milliseconds
    ///
    /// # Returns
    ///
    /// * `Ok(Some(OtaC2D))` - Received packet
    /// * `Ok(None)` - No packet received within timeout
    /// * `Err(ConnectionError)` - An error occurred during reception
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

    /// Receives a packet from the controller
    ///
    /// # Arguments
    ///
    /// * `timeout_millis` - Optional timeout in milliseconds
    ///
    /// # Returns
    ///
    /// * `Ok(Some(OtaC2D))` - Received packet
    /// * `Ok(None)` - No packet received within timeout
    /// * `Err(ConnectionError)` - An error occurred during reception
    #[allow(unreachable_code)]
    #[allow(unused_variables)]
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

    /// Sends an unreliable log message to the controller
    ///
    /// # Arguments
    ///
    /// * `level` - Log level of the message
    /// * `message` - The message to send
    ///
    /// # Returns
    ///
    /// * `Ok(())` - Message was sent successfully
    /// * `Err(ConnectionError)` - An error occurred during transmission
    #[track_caller]
    pub fn log_unreliable<S: Display>(
        &mut self,
        level: log::Level,
        message: S,
    ) -> Result<(), ConnectionError> {
        let location = core::panic::Location::caller();
        let _ = self.network.as_mut().unwrap().poll();
        let mut file_name = location.file();
        if let Some(find) = file_name.rfind("/") {
            file_name = &file_name[find + 1..];
        }
        self.send(OtaD2CUnreliable::LogMessage {
            level,
            message: format!("[{}:{}] {}", file_name, location.column(), message),
        })
    }

    /// Sends a log message to the controller, requiring acknowledgment
    ///
    /// # Arguments
    ///
    /// * `level` - Log level of the message
    /// * `message` - The message to send
    ///
    /// # Returns
    ///
    /// * `Ok(())` - Message was sent successfully
    /// * `Err(ConnectionError)` - An error occurred during transmission
    #[track_caller]
    pub fn log_reliable<S: Display>(
        &mut self,
        level: log::Level,
        message: S,
    ) -> Result<(), ConnectionError> {
        let location = core::panic::Location::caller();
        let _ = self.network.as_mut().unwrap().poll();
        let mut file_name = location.file();
        if let Some(find) = file_name.rfind("/") {
            file_name = &file_name[find + 1..];
        }
        self.send(OtaD2CTransport::LogMessage {
            level,
            message: format!("[{}:{}] {}", file_name, location.column(), message),
        })
    }
}

impl Drop for ControllerConnection {
    /// Cleans up resources when the connection is dropped
    fn drop(&mut self) {
        drop(self.channel.take());
        drop(self.network.take());
        drop(self.udp_handle.take());
    }
}
