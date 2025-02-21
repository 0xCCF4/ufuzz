use fuzzer_data::{OTAControllerToDevice, OTADeviceToController};
use fuzzer_master::device_connection::DeviceConnection;
use fuzzer_master::fuzzer_node_bridge::FuzzerNodeInterface;

#[tokio::main]
async fn main() {
    env_logger::init();

    let interface = FuzzerNodeInterface::new("http://192.168.0.6:8000");
    let mut udp = DeviceConnection::new("192.168.0.44:4444").await.unwrap();

    udp.send(&OTAControllerToDevice::GetCapabilities).await.unwrap();
    udp.send(&OTAControllerToDevice::StartGeneticFuzzing {
        seed: 0,
        evolutions: 1,
        random_mutation_chance: 0.01,
        code_size: 10,
        keep_best_x_solutions: 10,
        population_size: 100,
        random_solutions_each_generation: 2,
    }).await.unwrap();

    println!("{:?}", udp.receive().await.unwrap());


}
