//! Device Connection Module
//!
//! This module provides functionality for establishing and managing network connections
//! with fuzzing devices. It implements a reliable UDP-based communication protocol
//!
//! The module provides a high-level interface for sending and receiving messages
//! between the fuzzer master and devices, handling all the low-level details
//! of network communication and protocol management.

use fuzzer_data::{
    Ota, OtaC2D, OtaC2DTransport, OtaC2DUnreliable, OtaD2C, OtaD2CTransport, OtaD2CUnreliable,
    OtaPacket, MAX_FRAGMENT_SIZE, MAX_PAYLOAD_SIZE,
};
use log::{debug, error, info, trace, warn, Level};
use performance_timing::{track_time, TimeMeasurement};
use rand::random;
use std::collections::VecDeque;
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::sync::mpsc::Receiver;
use tokio::task::JoinHandle;
use tokio::time::Instant;

/// Errors that can occur during device connection operations
#[derive(Debug)]
pub enum DeviceConnectionError {
    /// I/O error during network operations
    Io(io::Error),
    /// Error during message serialization/deserialization
    Serde(String),
    /// End of file/connection
    Eof,
    /// Message exceeds maximum allowed size
    MessageTooLong(usize),
    /// No acknowledgment received for reliable message
    NoAckReceived,
}

impl DeviceConnectionError {
    /// Checks if the error is a timeout error
    pub fn is_timeout(&self) -> bool {
        matches!(self, DeviceConnectionError::NoAckReceived)
    }
}

impl Display for DeviceConnectionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            DeviceConnectionError::Io(e) => write!(f, "IO error: {}", e),
            DeviceConnectionError::Serde(e) => write!(f, "Serde error: {}", e),
            DeviceConnectionError::Eof => write!(f, "EOF"),
            DeviceConnectionError::MessageTooLong(len) => write!(f, "Message too long: {}", len),
            DeviceConnectionError::NoAckReceived => write!(f, "No ack received"),
        }
    }
}

impl Error for DeviceConnectionError {}

impl From<io::Error> for DeviceConnectionError {
    fn from(error: io::Error) -> Self {
        DeviceConnectionError::Io(error)
    }
}

impl From<String> for DeviceConnectionError {
    fn from(error: String) -> Self {
        DeviceConnectionError::Serde(error)
    }
}

/// Manages the connection to the fuzzing agent
///
/// This structure handles all aspects of communication with a device,
/// including connection management, message sending and receiving, and
/// protocol state tracking.
pub struct DeviceConnection {
    /// UDP socket for communication
    socket: Arc<UdpSocket>,
    /// Channel receiver for incoming messages
    receiver: Receiver<OtaD2C>,
    /// Handle to the receiver thread
    receiver_thread: Option<JoinHandle<()>>,

    /// Number of retransmission attempts
    resent_attempts: u8,
    /// Timeout for acknowledgment in milliseconds
    ack_timeout: Duration,
    /// Timeout for fragment reception in milliseconds
    fragment_timeout: Duration,

    /// Queue for received messages
    virtual_receive_queue: VecDeque<OtaD2C>,

    /// Next sequence number for transmitted messages
    sequence_number_tx: u64,
    /// Current session identifier
    session: u16,
}

impl DeviceConnection {
    /// Creates a new connection to a fuzzing agent
    ///
    /// # Arguments
    ///
    /// * `target` - Socket address of the target device
    ///
    /// # Returns
    ///
    /// * `Result<DeviceConnection, DeviceConnectionError>` - New connection or error
    pub async fn new<A: ToSocketAddrs>(
        target: A,
    ) -> Result<DeviceConnection, DeviceConnectionError> {
        let address = target
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::AddrNotAvailable, "No address found"))?;

        let socket = UdpSocket::bind(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
            address.port(),
        ))
        .await?;
        socket.connect(address).await?;

        let socket = Arc::new(socket);
        let socket_clone = Arc::clone(&socket);

        let (sender, receiver) = tokio::sync::mpsc::channel(100);

        let session = random();
        println!("Session: {}", session);

        {
            // ice-breaker
            let send_buf = OtaC2D::Unreliable(OtaC2DUnreliable::NOP)
                .serialize()
                .expect("must work");

            for _ in 0..10 {
                if let Err(err) = socket_clone.send(&send_buf).await {
                    error!("Error ice-breaking: {}", err)
                }
            }
        }

        let thread = tokio::spawn(async move {
            let mut buffer = [0u8; 4096];

            let mut rx_sequence_number = 0;

            // ice-breaker
            let ice_breaker_send_buf = OtaC2D::Unreliable(OtaC2DUnreliable::NOP)
                .serialize()
                .expect("must work");

            let mut last_ice_break = Instant::now();

            let mut last_packet = Instant::now();

            loop {
                let now = Instant::now();

                if (now - last_ice_break).as_secs() > 60 {
                    last_ice_break = now;
                    if let Err(err) = socket_clone.send(&ice_breaker_send_buf).await {
                        error!("Error ice-breaking: {}", err)
                    }
                }

                match socket_clone.recv(&mut buffer).await {
                    Ok(count) => {
                        let data: OtaD2C = match OtaD2C::deserialize(&buffer[..count]) {
                            Ok(d) => d,
                            Err(e) => {
                                error!("Error parsing JSON: {:?}", e);
                                continue;
                            }
                        };

                        if let Some(ack) = data.ack() {
                            let string = &OtaC2D::Unreliable(ack).serialize().expect("must work");
                            if let Err(err) = socket_clone.send(&string).await {
                                error!("Failed to send ack: {:?}", err);
                            }
                        }

                        if let Ota::Transport {
                            session: packet_session,
                            id,
                            content,
                            ..
                        } = &data
                        {
                            if let OtaD2CTransport::ResetSession = content {
                                trace!("Reset network session");
                                rx_sequence_number = 0;
                                last_packet = Instant::now();
                                continue;
                            } else if session != *packet_session {
                                warn!("Received packet from wrong session: {}", packet_session);
                                println!("Packet: {:?}", data);
                                continue;
                            } else if *id < rx_sequence_number {
                                if last_packet.elapsed() < Duration::from_secs(60) {
                                    warn!("Received packet with old sequence number: {}", id);
                                    continue;
                                } else {
                                    trace!("Received packet with old sequence number: {}. Resetting SEQ", id);
                                }
                            } else if *id == rx_sequence_number {
                                warn!("Received packet with same sequence number: {}", id);
                                continue;
                            }
                            rx_sequence_number = *id;
                            last_packet = Instant::now();
                        }

                        if let Ota::Transport {
                            content: OtaD2CTransport::LogMessage { level, message },
                            ..
                        }
                        | Ota::Unreliable(OtaD2CUnreliable::LogMessage { level, message }) = data
                        {
                            match level {
                                Level::Error => error!(target: "device", "{}", message),
                                Level::Warn => warn!(target: "device", "{}", message),
                                Level::Info => info!(target: "device", "{}", message),
                                Level::Debug => debug!(target: "device", "{}", message),
                                Level::Trace => debug!(target: "device", "{}", message),
                            }

                            continue; // do not send to frontend
                        }

                        if let Err(_) = sender.send(data).await {
                            break; // shutdown
                        }
                    }
                    Err(e) => {
                        if e.kind() == io::ErrorKind::WouldBlock {
                            tokio::time::sleep(Duration::from_millis(10)).await;
                            continue;
                        }
                        error!("Error receiving data: {:?}", e);
                    }
                }
            }
        });

        Ok(DeviceConnection {
            socket,
            receiver_thread: Some(thread),
            receiver,
            virtual_receive_queue: VecDeque::new(),
            ack_timeout: Duration::from_millis(200),
            fragment_timeout: Duration::from_secs(1),
            resent_attempts: 10,

            sequence_number_tx: 0,
            session,
        })
    }

    /// Sends a packet to the device
    ///
    /// # Arguments
    ///
    /// * `data` - The packet to send
    ///
    /// # Returns
    ///
    /// * `Result<(), DeviceConnectionError>` - Success or error
    #[allow(unreachable_code)]
    #[track_time("host::net::send")]
    pub async fn send<Packet: OtaPacket<OtaC2DUnreliable, OtaC2DTransport>>(
        &mut self,
        data: Packet,
    ) -> Result<(), DeviceConnectionError> {
        let packet = if data.reliable_transport() {
            self.sequence_number_tx += 1;

            data.to_packet(self.sequence_number_tx, self.session)
        } else {
            data.to_packet(0, 0)
        };

        let buf = packet.serialize().expect("Always works");

        if buf.len() as u64 > MAX_FRAGMENT_SIZE {
            // fragment

            return Err(DeviceConnectionError::MessageTooLong(buf.len())); // todo

            if let Ota::Transport { id, session, .. } = &mut packet {
                *id = 0;
                *session = 0;
            }

            drop(buf);
            let buf = packet.serialize().expect("Always works");

            let chunks = buf
                .chunks(MAX_FRAGMENT_SIZE as usize - 128)
                .collect::<Vec<&[u8]>>();
            for (i, chunk) in chunks.iter().enumerate() {
                self.sequence_number_tx += 1;
                let packet = OtaC2D::ChunkedTransport {
                    session: self.session,
                    id: self.sequence_number_tx,
                    fragment: i as u64,
                    total_fragments: chunks.len() as u64,
                    content: chunk.to_vec(),
                };
                let buf = packet.serialize().expect("Always works");
                self.send_native(&buf, true).await?;
            }
            Ok(())
        } else {
            // just send
            self.send_native(&buf, matches!(packet, OtaC2D::Transport { .. }))
                .await
        }
    }

    /// Sends raw data to the device
    ///
    /// # Arguments
    ///
    /// * `data` - The data to send
    /// * `requires_ack` - Whether to wait for acknowledgment
    ///
    /// # Returns
    ///
    /// * `Result<(), DeviceConnectionError>` - Success or error
    async fn send_native(
        &mut self,
        data: &[u8],
        requires_ack: bool,
    ) -> Result<(), DeviceConnectionError> {
        let mut virtual_receive_buffer = VecDeque::new();

        let mut status = None;
        'attempt_loop: for _attempt in 0..self.resent_attempts {
            // initial packet sending
            match self.socket.send(data).await {
                Ok(count) => {
                    if count != data.len() {
                        status = Some(Err(DeviceConnectionError::Eof));
                        break 'attempt_loop;
                    }
                }
                Err(e) => {
                    status = Some(Err(DeviceConnectionError::Io(e)));
                    break 'attempt_loop;
                }
            }

            // check if requires ack
            if !requires_ack {
                // does not require ack
                status = Some(Ok(()));
                break 'attempt_loop;
            }

            // wait for ack
            while let Some(received_packet) = self.receive(Some(self.ack_timeout)).await {
                if let OtaD2C::Unreliable(OtaD2CUnreliable::Ack(sequence_number)) = received_packet
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
                        // warn!("Received ack for past sequence number: {}", sequence_number);
                    }
                } else {
                    // received other package
                    virtual_receive_buffer.push_back(received_packet);
                }
            }
        }

        // requeue the received packets
        for packet in virtual_receive_buffer.into_iter().rev() {
            self.virtual_receive_queue.push_front(packet);
        }

        status.unwrap_or(Err(DeviceConnectionError::NoAckReceived))
    }

    /// Receives raw data from the device
    ///
    /// # Arguments
    ///
    /// * `timeout` - Optional timeout duration
    ///
    /// # Returns
    ///
    /// * `Option<OtaD2C>` - Received packet or None
    async fn receive_native(&mut self, timeout: Option<Duration>) -> Option<OtaD2C> {
        let now = Instant::now();
        loop {
            if let Some(data) = self.virtual_receive_queue.pop_front() {
                return Some(data);
            }
            if let Some(data) = self.receiver.try_recv().ok() {
                return Some(data);
            }
            if let Some(timeout) = timeout {
                if now.elapsed() > timeout {
                    return None;
                }
            } else {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// Receives a packet from the device
    ///
    /// # Arguments
    ///
    /// * `timeout` - Optional timeout duration
    ///
    /// # Returns
    ///
    /// * `Option<OtaD2C>` - Received packet or None
    #[allow(unreachable_code)]
    #[allow(unused_variables)]
    #[track_time("host::net::receive::raw")]
    pub async fn receive(&mut self, timeout: Option<Duration>) -> Option<OtaD2C> {
        let initial_packet = match self.receive_native(timeout).await {
            Some(packet) => packet,
            None => return None,
        };

        match initial_packet {
            OtaD2C::Transport { .. } => Some(initial_packet),
            OtaD2C::Unreliable(_) => Some(initial_packet),
            OtaD2C::ChunkedTransport {
                session,
                id,
                content,
                fragment,
                total_fragments,
            } => {
                return None; // todo

                if total_fragments.saturating_mul(MAX_FRAGMENT_SIZE as u64)
                    > MAX_PAYLOAD_SIZE as u64
                {
                    error!("Fragmented packet too large");
                    return None;
                }

                if fragment != 0 {
                    error!("Received fragment without initial packet");
                    return None;
                }

                let mut packet_content: Vec<u8> = content;

                trace!("Received chunked packet: {:?}", total_fragments);

                for i in 1..total_fragments {
                    trace!("{}", i);
                    let mut received = false;
                    while let Some(received_packet) =
                        { self.receive_native(Some(self.fragment_timeout)).await }
                    {
                        if let OtaD2C::ChunkedTransport {
                            session: received_session,
                            id: received_id,
                            content: received_content,
                            fragment: received_fragment,
                            total_fragments: received_total_fragments,
                        } = received_packet
                        {
                            trace!(" -> {:?}", received_content.len());
                            if session != received_session {
                                error!("Received packet with new session: {}", received_session);
                                return None;
                            }
                            if i != received_fragment {
                                error!(
                                    "Received packet with wrong fragment: {}",
                                    received_fragment
                                );
                                return None;
                            }
                            packet_content.extend(received_content);
                            received = true;
                            break;
                        } else {
                            // received other package
                            trace!("Dropped packet: {:?}", received_packet);
                        }
                    }
                    if !received {
                        error!("Failed to receive fragment: {}", i);
                        return None;
                    }
                }

                let data: OtaD2C = match OtaD2C::deserialize(&packet_content) {
                    Ok(data) => data,
                    Err(e) => {
                        error!("Failed to deserialize data: {:?}", e);
                        return None;
                    }
                };
                Some(data)
            }
        }
    }

    /// Receives a packet matching a filter condition. Buffers all other packets received while waiting for a match.
    ///
    /// # Arguments
    ///
    /// * `filter` - Function to filter received packets
    /// * `timeout` - Optional timeout duration
    ///
    /// # Returns
    ///
    /// * `Result<Option<OtaD2C>, DeviceConnectionError>` - Matching packet or error
    #[track_time("host::net::receive")]
    pub async fn receive_packet<F: Fn(&OtaD2C) -> bool>(
        &mut self,
        filter: F,
        timeout: Option<Duration>,
    ) -> Result<Option<OtaD2C>, DeviceConnectionError> {
        let now = Instant::now();
        let mut result = None;
        let mut queue = VecDeque::with_capacity(self.virtual_receive_queue.len() + 3);

        'outer: loop {
            while let Some(data) = self.receive(timeout).await {
                if filter(&data) {
                    result = Some(data);
                    break 'outer;
                }
                queue.push_back(data);
            }

            if let Some(timeout) = timeout {
                if now.elapsed() > timeout {
                    break;
                }
            } else {
                break;
            }

            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        for data in queue.into_iter().rev() {
            self.virtual_receive_queue.push_front(data);
        }

        Ok(result)
    }

    /// Flushes all pending received packets
    ///
    /// # Arguments
    ///
    /// * `timeout` - Optional timeout duration
    #[track_time("host::net::flush")]
    pub async fn flush_read(&mut self, timeout: Option<Duration>) {
        let timing = TimeMeasurement::begin("net");
        match timeout {
            None => while let Some(_) = self.receive(None).await {},
            Some(timeout) => {
                let now = Instant::now();
                loop {
                    while let Some(_) = self.receive(None).await {}

                    if now.elapsed() > timeout {
                        break;
                    }

                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
        }
        let _ = timing.stop();
    }
}

impl Drop for DeviceConnection {
    /// Cleans up resources when the connection is dropped
    fn drop(&mut self) {
        self.receiver.close();
        if let Some(thread) = self.receiver_thread.take() {
            thread.abort();
            drop(thread)
        }
    }
}
