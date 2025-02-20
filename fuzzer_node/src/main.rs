#[macro_use] extern crate rocket;

use std::{panic, process};
use std::ops::Deref;
use std::process::{Command};
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};
use lazy_static::lazy_static;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::Deserialize;
use rocket::serde::json::Json;
use fuzzer_data::{OverTheAirCommunicationControllerToDevice, OverTheAirCommunicationDeviceToController};
use serde::Serialize;

lazy_static! {
    static ref CMD: Vec<String> = {
        let env = std::env::var("CMD").unwrap_or_else(|_| "device_control".to_string());
        let env = env.split_whitespace().map(|v|v.to_string()).collect::<Vec<String>>();

        env
    };

    static ref GLOBALS: Globals = Globals::default();
}

fn execute_command(mut command: Command) -> Result<status::Custom<String>, status::Custom<String>> {
    match command.output() {
        Ok(output) => {
            if output.status.success() {
                Ok(status::Custom(Status::Ok, String::from_utf8_lossy(&output.stdout).to_string()))
            } else {
                Err(status::Custom(Status::InternalServerError, String::from_utf8_lossy(&output.stderr).to_string()))
            }
        },
        Err(_) => Err(status::Custom(Status::InternalServerError, "Failed to execute command".into())),
    }
}

#[post("/powerbutton/long")]
fn power_button_long() -> Result<status::Custom<String>, status::Custom<String>> {
    let mut command = Command::new(&CMD[0]);
    command.args(&CMD[1..]);
    command.arg("powerbutton");
    command.arg("long");
    execute_command(command)
}

#[post("/powerbutton/short")]
fn power_button_short() -> Result<status::Custom<String>, status::Custom<String>> {
    let mut command = Command::new(&CMD[0]);
    command.args(&CMD[1..]);
    command.arg("powerbutton");
    command.arg("short");
    execute_command(command)
}

pub struct Globals {
    pub last_heartbeat: Mutex<Arc<std::time::Instant>>,
    pub tx_queue_sender: Mutex<Sender<OverTheAirCommunicationControllerToDevice>>,
    pub tx_queue_receiver: Mutex<Receiver<OverTheAirCommunicationControllerToDevice>>,
    pub rx_queue_sender: Mutex<Sender<OverTheAirCommunicationDeviceToController>>,
    pub rx_queue_receiver: Mutex<Receiver<OverTheAirCommunicationDeviceToController>>,
}

impl Default for Globals {
    fn default() -> Self {
        let (tx_sender, tx_receiver) = std::sync::mpsc::channel();
        let (rx_sender, rx_receiver) = std::sync::mpsc::channel();
        Self {
            last_heartbeat: Mutex::new(Arc::new(Instant::now()-std::time::Duration::new(1*60*60, 0))),
            tx_queue_sender: Mutex::new(tx_sender),
            tx_queue_receiver: Mutex::new(tx_receiver),
            rx_queue_sender: Mutex::new(rx_sender),
            rx_queue_receiver: Mutex::new(rx_receiver),
        }
    }
}

#[get("/last", format = "json")]
fn last_heartbeat() -> Result<status::Custom<Json<u64>>, status::Custom<String>> {
    let now = Instant::now();

    let difference = {
        let guard = GLOBALS.last_heartbeat.lock().unwrap();
        let last_heartbeat = guard.deref().deref();
        now.duration_since(*last_heartbeat)
    };

    Ok(status::Custom(Status::Accepted, difference.as_secs().into()))
}

#[get("/alive", format = "json")]
fn alive() -> Result<status::Custom<Json<bool>>, status::Custom<String>> {
    Ok(status::Custom(Status::Accepted, true.into()))
}

#[derive(Deserialize)]
#[serde(crate = "rocket::serde")]
struct SendToDevice(OverTheAirCommunicationControllerToDevice);

#[derive(Serialize)]
#[serde(crate = "rocket::serde")]
struct ReceiveFromDevice(OverTheAirCommunicationDeviceToController);

#[post("/send", format = "json", data = "<data>")]
fn send(data: Json<SendToDevice>) -> Result<status::Custom<()>, status::Custom<String>> {
    match data.0.0 {
        OverTheAirCommunicationControllerToDevice::Ack(_) => {
            Err(status::Custom(Status::BadRequest, "Cannot send Ack to device".into()))
        },
        x => {
            let guard = GLOBALS.tx_queue_sender.lock().unwrap();
            guard.send(x).unwrap();
            Ok(status::Custom(Status::Ok, ()))
        }
    }
}

#[get("/receive", format = "json")]
fn receive() -> Result<Json<ReceiveFromDevice>, status::Custom<String>> {
    let guard = GLOBALS.rx_queue_receiver.lock().unwrap();
    let data = guard.recv_timeout(Duration::new(1,0)).unwrap();
    Ok(Json(ReceiveFromDevice(data)))
}

#[launch]
fn rocket() -> _ {
    let orig_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        // invoke the default handler and exit the process
        orig_hook(panic_info);
        process::exit(1);
    }));

    let _thread = std::thread::spawn(udp_handler);
    rocket::build().mount("/", routes![power_button_long, power_button_short, last_heartbeat, alive, send, receive])
}

fn udp_handler() {
    let bind = std::env::var("BIND").unwrap_or_else(|_| "0.0.0.0:4444".to_string());
    let target = std::env::var("TARGET").unwrap_or_else(|_| "192.168.0:4444".to_string());

    let udp_socket = std::net::UdpSocket::bind(bind).expect("Failed to bind to UDP socket");
    udp_socket.set_read_timeout(Some(std::time::Duration::from_secs(1))).expect("Failed to set read timeout");
    udp_socket.connect(target).expect("Failed to connect to UDP socket");

}