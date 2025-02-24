#[macro_use]
extern crate rocket;

use lazy_static::lazy_static;
use rocket::config::Config;
use rocket::figment::providers::Format;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rocket::Shutdown;
use std::process::Command;

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

#[post("/power_button_long")]
fn power_button_long() -> Result<status::Custom<String>, status::Custom<String>> {
    let mut command = Command::new(&CMD[0]);
    command.args(&CMD[1..]);
    command.arg("power_button");
    command.arg("long");
    execute_command(command)
}

#[post("/power_button_short")]
fn power_button_short() -> Result<status::Custom<String>, status::Custom<String>> {
    let mut command = Command::new(&CMD[0]);
    command.args(&CMD[1..]);
    command.arg("power_button");
    command.arg("short");
    execute_command(command)
}

#[post("/skip_bios")]
fn skip_bios() -> Result<status::Custom<String>, status::Custom<String>> {
    let mut command = Command::new(&CMD[0]);
    command.args(&CMD[1..]);
    command.arg("skip_bios");
    execute_command(command)
}

#[get("/alive", format = "json")]
fn alive() -> Result<status::Custom<Json<bool>>, status::Custom<String>> {
    Ok(status::Custom(Status::Accepted, true.into()))
}

#[get("/shutdown")]
fn shutdown(shutdown: Shutdown) -> Result<status::Custom<String>, status::Custom<String>> {
    shutdown.notify();
    Ok(status::Custom(Status::Ok, "Shutting down".to_string()))
}

#[launch]
fn rocket() -> _ {
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
            shutdown
        ],
    )
}
