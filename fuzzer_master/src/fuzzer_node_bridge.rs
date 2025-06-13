//! Fuzzer Node Bridge Module
//!
//! This module provides a bridge interface for communicating with fuzzing watchdog node.

use crate::wait_for_pi;
use reqwest::Client;
use std::time::Duration;

/// Interface for communicating with fuzzing watchdog
///
/// This struct manages HTTP communication with fuzzing nodes, providing methods
/// for power control and node status monitoring.
pub struct FuzzerNodeInterface {
    /// Host address of the fuzzing node
    host: String,
    /// HTTP client for making requests
    client: Client,
}

impl FuzzerNodeInterface {
    /// Creates a new fuzzer node interface
    ///
    /// # Arguments
    ///
    /// * `host` - Host address of the fuzzing node
    ///
    /// # Returns
    ///
    /// A new `FuzzerNodeInterface` instance
    pub fn new(host: &str) -> Self {
        FuzzerNodeInterface {
            host: host.to_string(),
            client: Self::client(),
        }
    }

    /// Creates a new HTTP client with configured timeouts
    ///
    /// # Returns
    ///
    /// A configured `Client` instance
    pub fn client() -> Client {
        Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(20))
            .build()
            .unwrap()
    }

    /// Attempts to skip the BIOS screen
    ///
    /// # Returns
    ///
    /// * `Result<bool, reqwest::Error>` indicating success or failure
    pub async fn skip_bios(&self) -> Result<bool, reqwest::Error> {
        wait_for_pi(&self).await;
        let url = format!("{}/skip_bios", self.host);
        let response = self.client.post(&url).send().await?;
        Ok(response.status().is_success())
    }

    /// Sends a long press of the power button
    ///
    /// # Returns
    ///
    /// * `Result<bool, reqwest::Error>` indicating success or failure
    pub async fn power_button_long(&self) -> Result<bool, reqwest::Error> {
        wait_for_pi(&self).await;
        let url = format!("{}/power_button_long", self.host);
        let response = self.client.post(&url).send().await?;
        if response.status().is_success() {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        Ok(response.status().is_success())
    }

    /// Sends a short press of the power button
    ///
    /// # Returns
    ///
    /// * `Result<bool, reqwest::Error>` indicating success or failure
    pub async fn power_button_short(&self) -> Result<bool, reqwest::Error> {
        wait_for_pi(&self).await;
        let url = format!("{}/power_button_short", self.host);
        let response = self.client.post(&url).send().await?;
        if response.status().is_success() {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        Ok(response.status().is_success())
    }

    /// Checks if the node is alive and responding
    ///
    /// # Returns
    ///
    /// * `Result<bool, reqwest::Error>` indicating if the node is alive
    pub async fn alive(&self) -> Result<bool, reqwest::Error> {
        let url = format!("{}/alive", self.host);
        let response = self.client.get(&url).send().await?;
        Ok(response.status().is_success())
    }
}
