use reqwest::Client;

pub struct FuzzerNodeInterface {
    host: String,
    client: Client,
}

impl FuzzerNodeInterface {
    pub fn new(host: &str) -> Self {
        FuzzerNodeInterface {
            host: host.to_string(),
            client: Client::new(),
        }
    }

    pub async fn power_button_long(&self) -> Result<bool, reqwest::Error> {
        let url = format!("{}/powerbutton/long", self.host);
        let response = self.client.post(&url).send().await?;
        Ok(response.status().is_success())
    }

    pub async fn power_button_short(&self) -> Result<bool, reqwest::Error> {
        let url = format!("{}/powerbutton/short", self.host);
        let response = self.client.post(&url).send().await?;
        Ok(response.status().is_success())
    }

    pub async fn alive(&self) -> Result<bool, reqwest::Error> {
        let url = format!("{}/alive", self.host);
        let response = self.client.get(&url).send().await?;
        Ok(response.status().is_success())
    }
}