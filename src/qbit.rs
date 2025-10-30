use crate::error::{QbitError, Result};
use reqwest::{header, Client, Url};
use serde::Deserialize;
use std::time::Duration;
use tracing::{debug, info, warn};

#[derive(Clone)]
pub struct QbitClient {
    client: Client,
    base_url: Url,
}

#[derive(Debug, Deserialize)]
pub struct Preferences {
    pub listen_port: u16,
}

impl QbitClient {
    pub fn new(mut base_url: Url) -> Result<Self> {
        if base_url.path().is_empty() {
            base_url.set_path("/");
        }

        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::REFERER,
            header::HeaderValue::from_str(base_url.as_str())?,
        );

        let client = Client::builder()
            .default_headers(headers)
            .cookie_store(true)
            .timeout(Duration::from_secs(10))
            .user_agent("qb-port-sync")
            .build()?;

        Ok(Self { client, base_url })
    }

    pub async fn login(&self, username: &str, password: &str) -> Result<()> {
        let url = self.endpoint("api/v2/auth/login")?;
        let response = self
            .client
            .post(url)
            .form(&[("username", username), ("password", password)])
            .send()
            .await?;

        let status = response.status();
        let body = response.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(QbitError::UnexpectedResponse {
                status,
                message: body,
            }
            .into());
        }

        if body.trim() != "Ok." {
            return Err(QbitError::Auth(body).into());
        }

        info!("authenticated with qBittorrent Web API");
        Ok(())
    }

    pub async fn set_listen_port(&self, port: u16) -> Result<()> {
        let url = self.endpoint("api/v2/app/setPreferences")?;
        let json_payload = build_port_payload(port);
        let response = self
            .client
            .post(url)
            .header(
                header::CONTENT_TYPE,
                header::HeaderValue::from_static("application/x-www-form-urlencoded"),
            )
            .form(&[("json", json_payload)])
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let message = response.text().await.unwrap_or_default();
            return Err(QbitError::UnexpectedResponse { status, message }.into());
        }

        debug!("requested qBittorrent to update listen port to {}", port);
        Ok(())
    }

    pub async fn update_listen_port(&self, port: u16) -> Result<()> {
        self.set_listen_port(port).await?;
        let prefs = self.get_preferences().await?;
        if prefs.listen_port != port {
            warn!(
                "qBittorrent listen port mismatch after update: expected {}, got {}",
                port, prefs.listen_port
            );
        } else {
            info!("qBittorrent listen port confirmed at {}", port);
        }
        Ok(())
    }

    pub async fn get_preferences(&self) -> Result<Preferences> {
        let url = self.endpoint("api/v2/app/preferences")?;
        let response = self.client.get(url).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let message = response.text().await.unwrap_or_default();
            return Err(QbitError::UnexpectedResponse { status, message }.into());
        }

        let prefs = response.json::<Preferences>().await?;
        Ok(prefs)
    }

    fn endpoint(&self, path: &str) -> Result<Url> {
        self.base_url
            .join(path)
            .map_err(|err| anyhow::anyhow!("invalid endpoint path {}: {}", path, err))
    }
}

fn build_port_payload(port: u16) -> String {
    serde_json::json!({ "listen_port": port }).to_string()
}

#[cfg(test)]
mod tests {
    use super::build_port_payload;

    #[test]
    fn payload_encodes_listen_port() {
        assert_eq!(build_port_payload(12345), "{\"listen_port\":12345}");
    }
}
