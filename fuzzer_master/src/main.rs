use fuzzer_master::fuzzer_node_bridge::FuzzerNodeInterface;

#[tokio::main]
async fn main() {
    let interface = FuzzerNodeInterface::new("http://192.168.0.6:8000");

    println!("{:?}", interface.power_button_long().await.unwrap());
}
