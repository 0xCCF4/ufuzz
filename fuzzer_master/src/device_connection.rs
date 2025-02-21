use fuzzer_data::{OTAControllerToDevice, OTADeviceToController};
use log::error;
use std::collections::VecDeque;
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
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
    tx_socket: UdpSocket,
    receiver: Receiver<OTADeviceToController>,
    receiver_thread: Option<JoinHandle<()>>,

    resent_attempts: u8,
    ack_timeout: Duration,

    virtual_receive_queue: VecDeque<OTADeviceToController>,
}

impl DeviceConnection {
    pub async fn new<A: ToSocketAddrs>(
        target: A,
    ) -> Result<DeviceConnection, DeviceConnectionError> {
        let address = target
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::AddrNotAvailable, "No address found"))?;

        let tx_socket = UdpSocket::bind("0.0.0.0:0").await?;
        tx_socket.connect(address).await?;

        let rx_socket = UdpSocket::bind(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
            address.port(),
        ))
        .await?;
        rx_socket.connect(address).await?;

        let (sender, receiver) = tokio::sync::mpsc::channel(100);

        let thread = tokio::spawn(async move {
            let mut buffer = [0u8; 4096];

            // ice breaker
            let send_buf_str =
                serde_json::to_string(&OTAControllerToDevice::NOP).expect("must work");
            let send_buf = send_buf_str.as_bytes();

            for _ in 0..10 {
                if let Err(err) = rx_socket.send(&send_buf).await {
                    error!("Error ice-breaking: {}", err)
                }
            }

            let mut last_ice_break = Instant::now();

            loop {
                let now = Instant::now();

                if (now - last_ice_break).as_secs() > 60 {
                    last_ice_break = now;
                    if let Err(err) = rx_socket.send(&send_buf).await {
                        error!("Error ice-breaking: {}", err)
                    }
                }

                match rx_socket.try_recv(&mut buffer) {
                    Ok(count) => {
                        let string = match std::str::from_utf8(&buffer[..count]) {
                            Ok(s) => s,
                            Err(e) => {
                                error!("Error converting buffer to string: {:?}", e);
                                continue;
                            }
                        };

                        let data: OTADeviceToController = match serde_json::from_str(string) {
                            Ok(d) => d,
                            Err(e) => {
                                error!("Error parsing JSON: {:?}", e);
                                continue;
                            }
                        };

                        if let Some(ack) = data.ack() {
                            let string = serde_json::to_string(&ack).expect("must work");
                            if let Err(err) = rx_socket.send(string.as_bytes()).await {
                                error!("Failed to send ack: {:?}", err);
                            }
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
            tx_socket,
            receiver_thread: Some(thread),
            receiver,
            virtual_receive_queue: VecDeque::new(),
            ack_timeout: Duration::from_millis(200),
            resent_attempts: 10,
        })
    }

    pub async fn send(
        &mut self,
        data: &OTAControllerToDevice,
    ) -> Result<(), DeviceConnectionError> {
        let string = serde_json::to_string(data).expect("Always works");
        let bytes = string.as_bytes();

        if bytes.len() > 4000 {
            return Err(DeviceConnectionError::MessageTooLong(bytes.len()));
        }

        let mut virtual_receive_buffer = VecDeque::new();

        let mut status = None;
        'attempt_loop: for _attempt in 0..self.resent_attempts {
            // initial packet sending
            match self.tx_socket.send(bytes).await {
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
            if !data.requires_ack() {
                // does not require ack
                status = Some(Ok(()));
                break 'attempt_loop;
            }

            // wait for ack
            while let Some(received_packet) = self.receive_timeout(self.ack_timeout).await {
                if let OTADeviceToController::Ack(ack_content) = received_packet {
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
            self.virtual_receive_queue.push_front(packet);
        }

        status.unwrap_or(Err(DeviceConnectionError::NoAckReceived))
    }

    pub fn receive(&mut self) -> Option<OTADeviceToController> {
        if let Some(data) = self.virtual_receive_queue.pop_front() {
            return Some(data);
        }

        self.receiver.try_recv().ok()
    }

    pub async fn receive_timeout(&mut self, timeout: Duration) -> Option<OTADeviceToController> {
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
