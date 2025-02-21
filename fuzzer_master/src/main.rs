use fuzzer_data::OTAControllerToDevice;
use fuzzer_master::device_connection::DeviceConnection;
use fuzzer_master::fuzzer_node_bridge::FuzzerNodeInterface;
use std::net::UdpSocket;

#[tokio::main]
async fn main() {
    env_logger::init();

    let interface = FuzzerNodeInterface::new("http://192.168.0.6:8000");
    let mut udp = DeviceConnection::new("192.168.0.44:4444").await.unwrap();
}
