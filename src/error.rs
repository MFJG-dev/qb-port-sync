use thiserror::Error;

pub type Result<T> = anyhow::Result<T>;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read config file: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse config file: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("no configuration file found; pass --config or create one in a standard location")]
    MissingConfig,
    #[error("missing qbittorrent password (set in config or QB_PORT_SYNC_QB_PASSWORD)")]
    MissingQbPassword,
    #[error("invalid forwarded port value: {0}")]
    #[allow(dead_code)]
    InvalidForwardedPort(String),
}

#[derive(Debug, Error)]
pub enum QbitError {
    #[error("authentication failed: {0}")]
    Auth(String),
    #[error("unexpected response status: {status} {message}")]
    UnexpectedResponse {
        status: reqwest::StatusCode,
        message: String,
    },
    #[error("failed to deserialize qBittorrent preferences: {0}")]
    Deserialize(#[from] serde_json::Error),
}

#[derive(Debug, Error)]
pub enum PortMapError {
    #[error("pcp mapping failed: {0}")]
    Pcp(String),
    #[error("nat-pmp mapping failed: {0}")]
    NatPmp(String),
    #[error("no supported port mapping methods succeeded")]
    Exhausted,
}
