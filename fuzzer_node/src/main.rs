//! # Fuzzer Node
//!
//! A web service that provides an API for controlling device power and skipping the BIOS screen.
//!
//! ## API Endpoints
//! - `POST /power_button_long`: Trigger a long power button press
//! - `POST /power_button_short`: Trigger a short power button press
//! - `POST /skip_bios`: Skip BIOS screen
//! - `GET /alive`: Check service health status
//! - `GET /shutdown`: Gracefully shutdown the service

#[macro_use]
extern crate rocket;

use lazy_static::lazy_static;
use rocket::config::Config;
use rocket::figment::providers::Format;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use sd_notify::NotifyState;
use std::process::Command;
use std::thread;
use std::time::Duration;

// Command configuration loaded from environment variables
lazy_static! {
    static ref CMD: Vec<String> = {
        let env = std::env::var("CMD")
            .unwrap_or_else(|_| "/run/current-system/sw/bin/device_control".to_string());
        let env = env
            .split_whitespace()
            .map(|v| v.to_string())
            .collect::<Vec<String>>();

        env
    };
}

/// Executes a system command and returns its output
///
/// # Arguments
/// * `command` - The command to execute
///
/// # Returns
/// * `Ok(status::Custom<String>)` - Command output on success
/// * `Err(status::Custom<String>)` - Error message on failure
fn execute_command(mut command: Command) -> Result<status::Custom<String>, status::Custom<String>> {
    println!("> {:?}", command);
    match command.output() {
        Ok(output) => {
            if output.status.success() {
                Ok(status::Custom(
                    Status::Ok,
                    String::from_utf8_lossy(&output.stdout).to_string(),
                ))
            } else {
                Err(status::Custom(
                    Status::InternalServerError,
                    String::from_utf8_lossy(&output.stderr).to_string(),
                ))
            }
        }
        Err(err) => Err(status::Custom(
            Status::InternalServerError,
            format!("Failed to execute command: {err:?}"),
        )),
    }
}

/// Handles long power button press requests
///
/// # Returns
/// * `Ok(status::Custom<String>)` - Success message
/// * `Err(status::Custom<String>)` - Error message
#[post("/power_button_long")]
fn power_button_long() -> Result<status::Custom<String>, status::Custom<String>> {
    let _ = sd_notify::notify(true, &[NotifyState::Ready]);
    let mut command = Command::new(&CMD[0]);
    command.args(&CMD[1..]);
    command.arg("power_button");
    command.arg("long");
    execute_command(command)
}

/// Handles short power button press requests
#[post("/power_button_short")]
fn power_button_short() -> Result<status::Custom<String>, status::Custom<String>> {
    let _ = sd_notify::notify(true, &[NotifyState::Ready]);
    let mut command = Command::new(&CMD[0]);
    command.args(&CMD[1..]);
    command.arg("power_button");
    command.arg("short");
    execute_command(command)
}

/// Handles BIOS skip requests
#[post("/skip_bios")]
fn skip_bios() -> Result<status::Custom<String>, status::Custom<String>> {
    let _ = sd_notify::notify(true, &[NotifyState::Ready]);
    let mut command = Command::new(&CMD[0]);
    command.args(&CMD[1..]);
    command.arg("skip_bios");
    execute_command(command)
}

/// Health check endpoint
#[get("/alive", format = "json")]
fn alive() -> Result<status::Custom<Json<bool>>, status::Custom<String>> {
    let _ = sd_notify::notify(true, &[NotifyState::Ready]);
    Ok(status::Custom(Status::Accepted, true.into()))
}

/// Runs the startup executable
#[post("/startup")]
fn startup() -> Result<status::Custom<String>, status::Custom<String>> {
    let _ = sd_notify::notify(true, &[NotifyState::Ready]);
    let mut command = Command::new(&CMD[0]);
    command.args(&CMD[1..]);
    command.arg("send_keys");
    command.arg("startup");
    execute_command(command)?;
    thread::sleep(Duration::from_secs(2));
    let mut command = Command::new(&CMD[0]);
    command.args(&CMD[1..]);
    command.arg("send_keys");
    command.arg("enter");
    execute_command(command)
}

/*
/// Graceful shutdown endpoint
#[get("/shutdown")]
fn shutdown(shutdown: Shutdown) -> Result<status::Custom<String>, status::Custom<String>> {
    shutdown.notify();
    Ok(status::Custom(Status::Ok, "Shutting down".to_string()))
}*/

/// Main method
#[launch]
fn rocket() -> _ {
    let _ = sd_notify::notify(true, &[NotifyState::Ready]);

    let config = Config::figment().merge(rocket::figment::providers::Toml::string(include_str!(
        "../rocket.toml"
    )));

    rocket::custom(config).mount(
        "/",
        routes![
            power_button_long,
            power_button_short,
            alive,
            skip_bios,
            //shutdown,
            startup,
        ],
    )
}
