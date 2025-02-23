use reqwest::Client;
use std::time::Duration;

pub struct FuzzerNodeInterface {
    host: String,
    client: Client,
}

impl FuzzerNodeInterface {
    pub fn new(host: &str) -> Self {
        FuzzerNodeInterface {
            host: host.to_string(),
            client: Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap(),
        }
    }

    pub async fn skip_bios(&self) -> Result<bool, reqwest::Error> {
        let url = format!("{}/skip_bios", self.host);
        let response = self.client.post(&url).send().await?;
        Ok(response.status().is_success())
    }

    pub async fn power_button_long(&self) -> Result<bool, reqwest::Error> {
        let url = format!("{}/power_button/long", self.host);
        let response = self.client.post(&url).send().await?;
        Ok(response.status().is_success())
    }

    pub async fn power_button_short(&self) -> Result<bool, reqwest::Error> {
        let url = format!("{}/power_button/short", self.host);
        let response = self.client.post(&url).send().await?;
        Ok(response.status().is_success())
    }

    pub async fn alive(&self) -> Result<bool, reqwest::Error> {
        let url = format!("{}/alive", self.host);
        let response = self.client.get(&url).send().await?;
        Ok(response.status().is_success())
    }
}
