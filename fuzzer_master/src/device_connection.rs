use fuzzer_data::{OTAControllerToDevice, OTADeviceToController};
use log::{error, info};
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
}

impl Display for DeviceConnectionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            DeviceConnectionError::Io(e) => write!(f, "IO error: {}", e),
            DeviceConnectionError::Serde(e) => write!(f, "Serde error: {}", e),
            DeviceConnectionError::Eof => write!(f, "EOF"),
            DeviceConnectionError::MessageTooLong(len) => write!(f, "Message too long: {}", len),
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
        })
    }

    pub async fn send(&self, data: &OTAControllerToDevice) -> Result<(), DeviceConnectionError> {
        let string = serde_json::to_string(data).expect("Always works");
        let bytes = string.as_bytes();

        if bytes.len() > 4000 {
            return Err(DeviceConnectionError::MessageTooLong(bytes.len()));
        }

        match self.tx_socket.send(bytes).await {
            Ok(count) => {
                if count == bytes.len() {
                    Ok(())
                } else {
                    Err(DeviceConnectionError::Eof)
                }
            }
            Err(e) => Err(DeviceConnectionError::Io(e)),
        }
    }

    pub fn receive(&mut self) -> Option<OTADeviceToController> {
        self.receiver.try_recv().ok()
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
