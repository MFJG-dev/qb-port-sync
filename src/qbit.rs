use crate::error::{QbitError, Result};
use reqwest::{header, Client, Url};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::convert::TryFrom;
use std::time::Duration;
use tracing::{debug, info, warn};

#[derive(Clone)]
pub struct QbitClient {
    client: Client,
    base_url: Url,
}

#[derive(Debug)]
pub struct PortUpdateResult {
    pub detected_port: u16,
    pub verified: bool,
    pub random_port: Option<bool>,
    pub upnp: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct NetworkInterfaceItem {
    name: String,
    #[serde(default)]
    interface: Option<String>,
    #[serde(default)]
    id: Option<String>,
}

impl QbitClient {
    pub fn new(mut base_url: Url) -> Result<Self> {
        if base_url.path().is_empty() {
            base_url.set_path("/");
        }

        let mut headers = header::HeaderMap::new();
        let referer = header::HeaderValue::from_str(base_url.as_str())?;
        headers.insert(header::REFERER, referer);
        let origin_string = origin_from_url(&base_url);
        let origin = header::HeaderValue::from_str(&origin_string)?;
        headers.insert(header::ORIGIN, origin);

        let client = Client::builder()
            .default_headers(headers)
            .cookie_store(true)
            .timeout(Duration::from_secs(15))
            .user_agent("qb-port-sync")
            .build()?;

        Ok(Self { client, base_url })
    }

    pub async fn login(&self, user: &str, pass: &str) -> Result<()> {
        let url = self.endpoint("api/v2/auth/login")?;
        let response = self
            .client
            .post(url)
            .form(&[("username", user), ("password", pass)])
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

    pub async fn set_listen_port(
        &self,
        port: u16,
        bind_interface: Option<&str>,
    ) -> Result<PortUpdateResult> {
        let mut payload = Map::new();
        payload.insert("listen_port".to_string(), json!(port));
        payload.insert("random_port".into(), Value::Bool(false));
        payload.insert("upnp".into(), Value::Bool(false));

        if let Some(interface) = bind_interface.map(str::trim).filter(|s| !s.is_empty()) {
            if let Some(selection) = self.resolve_interface(interface).await? {
                payload.insert("network_interface".into(), Value::String(selection.name));
                if let Some(id) = selection.id {
                    payload.insert("network_interface_id".into(), Value::String(id));
                }
            } else {
                warn!("requested bind interface '{}' not found on qBittorrent; continuing without binding", interface);
            }
        }

        self.post_preferences(payload).await?;
        let prefs = self.get_preferences().await?;
        let detected_port = prefs
            .get("listen_port")
            .and_then(Value::as_u64)
            .and_then(|v| u16::try_from(v).ok())
            .ok_or_else(|| anyhow::anyhow!("qBittorrent preferences missing listen_port"))?;
        let random_port = prefs.get("random_port").and_then(Value::as_bool);
        let upnp = prefs.get("upnp").and_then(Value::as_bool);

        let verified = detected_port == port;
        if verified {
            info!("qBittorrent listen port verified at {}", detected_port);
        } else {
            warn!(
                "qBittorrent listen port mismatch after update: expected {}, reported {}",
                port, detected_port
            );
        }

        Ok(PortUpdateResult {
            detected_port,
            verified,
            random_port,
            upnp,
        })
    }

    pub async fn get_preferences(&self) -> Result<Value> {
        let url = self.endpoint("api/v2/app/preferences")?;
        let response = self.client.get(url).send().await?;
        let status = response.status();
        if !status.is_success() {
            let message = response.text().await.unwrap_or_default();
            return Err(QbitError::UnexpectedResponse { status, message }.into());
        }
        let value = response.json::<Value>().await?;
        Ok(value)
    }

    async fn post_preferences(&self, payload: Map<String, Value>) -> Result<()> {
        let url = self.endpoint("api/v2/app/setPreferences")?;
        let response = self
            .client
            .post(url)
            .header(
                header::CONTENT_TYPE,
                header::HeaderValue::from_static("application/x-www-form-urlencoded"),
            )
            .form(&[("json", Value::Object(payload).to_string())])
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let message = response.text().await.unwrap_or_default();
            return Err(QbitError::UnexpectedResponse { status, message }.into());
        }

        debug!("submitted qBittorrent preference update");
        Ok(())
    }

    async fn resolve_interface(&self, requested: &str) -> Result<Option<InterfaceSelection>> {
        let items = match self.fetch_interfaces().await {
            Ok(items) => items,
            Err(err) => {
                warn!("failed to fetch qBittorrent network interfaces: {err:?}");
                return Ok(None);
            }
        };
        let matches: Vec<NetworkInterfaceItem> = items
            .into_iter()
            .filter(|item| matches_interface(item, requested))
            .collect();
        if let Some(item) = matches.first() {
            return Ok(Some(InterfaceSelection {
                name: item.name.clone(),
                id: item.id.clone().or_else(|| item.interface.clone()),
            }));
        }
        Ok(None)
    }

    async fn fetch_interfaces(&self) -> Result<Vec<NetworkInterfaceItem>> {
        let url = self.endpoint("api/v2/app/networkInterfaceList")?;
        let response = self.client.get(url).send().await?;
        let status = response.status();
        if !status.is_success() {
            let message = response.text().await.unwrap_or_default();
            return Err(QbitError::UnexpectedResponse { status, message }.into());
        }
        let list = response.json::<Vec<NetworkInterfaceItem>>().await?;
        Ok(list)
    }

    fn endpoint(&self, path: &str) -> Result<Url> {
        self.base_url
            .join(path)
            .map_err(|err| anyhow::anyhow!("invalid endpoint path {}: {}", path, err))
    }
}

fn matches_interface(item: &NetworkInterfaceItem, requested: &str) -> bool {
    let requested = requested.trim();
    if requested.is_empty() {
        return false;
    }
    item.name == requested
        || item
            .interface
            .as_deref()
            .map(|iface| iface == requested)
            .unwrap_or(false)
        || item
            .id
            .as_deref()
            .map(|id| id == requested)
            .unwrap_or(false)
}

fn origin_from_url(url: &Url) -> String {
    url.origin().unicode_serialization()
}

struct InterfaceSelection {
    name: String,
    id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::matches_interface;
    use super::NetworkInterfaceItem;

    #[test]
    fn interface_match_handles_aliases() {
        let item = NetworkInterfaceItem {
            name: "tun0".into(),
            interface: Some("tun0".into()),
            id: Some("{1234}".into()),
        };
        assert!(matches_interface(&item, "tun0"));
        assert!(matches_interface(&item, "{1234}"));
        assert!(!matches_interface(&item, "eth0"));
    }
}
