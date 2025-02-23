use fuzzer_data::{
    Ota, OtaC2D, OtaC2DTransport, OtaC2DUnreliable, OtaD2C, OtaD2CUnreliable, OtaPacket,
};
use log::{error, warn};
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

#[derive(Debug)]
pub enum DeviceConnectionError {
    Io(io::Error),
    Serde(serde_json::Error),
    Eof,
    MessageTooLong(usize),
    NoAckReceived,
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

impl From<serde_json::Error> for DeviceConnectionError {
    fn from(error: serde_json::Error) -> Self {
        DeviceConnectionError::Serde(error)
    }
}

pub struct DeviceConnection {
    socket: Arc<UdpSocket>,
    receiver: Receiver<OtaD2C>,
    receiver_thread: Option<JoinHandle<()>>,

    resent_attempts: u8,
    ack_timeout: Duration,

    virtual_receive_queue: VecDeque<OtaD2C>,

    sequence_number_tx: u64,
    session: u16,
}

impl DeviceConnection {
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
            let send_buf_str = serde_json::to_string(&OtaC2D::Unreliable(OtaC2DUnreliable::NOP))
                .expect("must work");
            let send_buf = send_buf_str.as_bytes();

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
            let ice_breaker_send_buf_str =
                serde_json::to_string(&OtaC2D::Unreliable(OtaC2DUnreliable::NOP))
                    .expect("must work");
            let ice_breaker_send_buf = ice_breaker_send_buf_str.as_bytes();

            let mut last_ice_break = Instant::now();

            loop {
                let now = Instant::now();

                if (now - last_ice_break).as_secs() > 60 {
                    last_ice_break = now;
                    if let Err(err) = socket_clone.send(&ice_breaker_send_buf).await {
                        error!("Error ice-breaking: {}", err)
                    }
                }

                match socket_clone.try_recv(&mut buffer) {
                    Ok(count) => {
                        let string = match std::str::from_utf8(&buffer[..count]) {
                            Ok(s) => s,
                            Err(e) => {
                                error!("Error converting buffer to string: {:?}", e);
                                continue;
                            }
                        };

                        let data: OtaD2C = match serde_json::from_str(string) {
                            Ok(d) => d,
                            Err(e) => {
                                error!("Error parsing JSON: {:?}", e);
                                continue;
                            }
                        };

                        if let Some(ack) = data.ack() {
                            let string =
                                serde_json::to_string(&OtaC2D::Unreliable(ack)).expect("must work");
                            if let Err(err) = socket_clone.send(string.as_bytes()).await {
                                error!("Failed to send ack: {:?}", err);
                            }
                        }

                        if let Ota::Transport {
                            session: packet_session,
                            id,
                            ..
                        } = &data
                        {
                            if session != *packet_session {
                                warn!("Received packet from wrong session: {}", packet_session);
                                continue;
                            } else if *id < rx_sequence_number {
                                warn!("Received packet with old sequence number: {}", id);
                                continue;
                            } else if *id == rx_sequence_number {
                                warn!("Received packet with same sequence number: {}", id);
                                continue;
                            }
                            rx_sequence_number = *id;
                        }

                        if let Err(_) = sender.send(data).await {
                            break; // shutdown
                        }
                    }
                    Err(e) => {
                        if e.kind() == io::ErrorKind::WouldBlock {
                            tokio::time::sleep(Duration::from_millis(500)).await;
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
            resent_attempts: 10,

            sequence_number_tx: 0,
            session,
        })
    }

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

        let string = serde_json::to_string(&packet).expect("Always works");
        let bytes = string.as_bytes();

        if bytes.len() > 4000 {
            return Err(DeviceConnectionError::MessageTooLong(bytes.len()));
        }

        let mut virtual_receive_buffer = VecDeque::new();

        let mut status = None;
        'attempt_loop: for _attempt in 0..self.resent_attempts {
            // initial packet sending
            match self.socket.send(bytes).await {
                Ok(count) => {
                    if count != bytes.len() {
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
            if !matches!(packet, Ota::Transport { .. }) {
                // does not require ack
                status = Some(Ok(()));
                break 'attempt_loop;
            }

            // wait for ack
            while let Some(received_packet) = self.receive_timeout(self.ack_timeout).await {
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

    pub fn receive(&mut self) -> Option<OtaD2C> {
        if let Some(data) = self.virtual_receive_queue.pop_front() {
            return Some(data);
        }

        self.receiver.try_recv().ok()
    }

    pub async fn receive_timeout(&mut self, timeout: Duration) -> Option<OtaD2C> {
        let now = Instant::now();
        loop {
            if let Some(data) = self.receive() {
                return Some(data);
            }
            if now.elapsed() > timeout {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

impl Drop for DeviceConnection {
    fn drop(&mut self) {
        self.receiver.close();
        if let Some(thread) = self.receiver_thread.take() {
            thread.abort();
            drop(thread)
        }
    }
}
