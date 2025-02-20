use std::io;
use std::net::{SocketAddr, ToSocketAddrs};
use log::error;
use tokio::net::UdpSocket;
use tokio::sync::mpsc::Receiver;
use tokio::task::{JoinHandle};
use fuzzer_data::{OverTheAirCommunicationControllerToDevice, OverTheAirCommunicationDeviceToController};

pub enum DeviceConnectionError {
    Io(io::Error),
    Serde(serde_json::Error),
    Eof,
    MessageTooLong(usize),
}

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
    receiver: Receiver<OverTheAirCommunicationDeviceToController>,
    receiver_thread: Option<JoinHandle<()>>,
}

impl DeviceConnection {
    pub async fn new<A: ToSocketAddrs>(target: A) -> Result<DeviceConnection, DeviceConnectionError> {
        let address = target.to_socket_addrs()?.next().ok_or_else(|| {
            io::Error::new(io::ErrorKind::AddrNotAvailable, "No address found")
        })?;
        let host = address.ip();

        let tx_socket = UdpSocket::bind(SocketAddr::new(host, 0)).await?;
        tx_socket.connect(address).await?;

        let rx_socket = UdpSocket::bind(address).await?;
        rx_socket.connect(address).await?;

        let (sender, receiver) = tokio::sync::mpsc::channel(100);

        let thread = tokio::spawn(async move {
            let mut buffer = [0u8; 4096];

            loop {
                match rx_socket.recv(&mut buffer).await {
                    Ok(count) => {
                        let string = match std::str::from_utf8(&buffer[..count]) {
                            Ok(s) => s,
                            Err(e) => {
                                error!("Error converting buffer to string: {:?}", e);
                                continue;
                            }
                        };

                        let data: OverTheAirCommunicationDeviceToController = match serde_json::from_str(string) {
                            Ok(d) => d,
                            Err(e) => {
                                error!("Error parsing JSON: {:?}", e);
                                continue;
                            }
                        };

                        if let Err(_) = sender.send(data).await {
                            break; // shutdown
                        }
                    },
                    Err(e) => {
                        eprintln!("Error receiving data: {:?}", e);
                    },
                }
            }
        });

        Ok(DeviceConnection {
            tx_socket,
            receiver_thread: Some(thread),
            receiver,
        })
    }

    pub async fn send(&self, data: &OverTheAirCommunicationControllerToDevice) -> Result<(), DeviceConnectionError> {
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
            },
            Err(e) => Err(DeviceConnectionError::Io(e)),
        }
    }

    pub fn receive(&mut self) -> Option<OverTheAirCommunicationDeviceToController> {
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