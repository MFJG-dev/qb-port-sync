use anyhow::Error;
use thiserror::Error;

pub type Result<T> = anyhow::Result<T>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCode {
    Success = 0,
    Transient = 1,
    Config = 2,
    Unsupported = 3,
}

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
    #[error("forwarded port path unavailable: {0}")]
    ForwardedPortUnavailable(String),
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
    #[cfg_attr(not(feature = "pcp"), allow(dead_code))]
    #[error("pcp mapping failed: {0}")]
    Pcp(String),
    #[error("pcp not supported: {0}")]
    PcpNotSupported(String),
    #[error("nat-pmp mapping failed: {0}")]
    NatPmp(String),
}

#[derive(Debug, Error)]
#[error("{0}")]
pub struct UnsupportedError(pub String);

impl UnsupportedError {
    pub fn new(message: impl Into<String>) -> Self {
        UnsupportedError(message.into())
    }
}

pub fn classify_error(err: &Error) -> ExitCode {
    if err.downcast_ref::<ConfigError>().is_some() {
        return ExitCode::Config;
    }

    if err.downcast_ref::<UnsupportedError>().is_some() {
        return ExitCode::Unsupported;
    }

    if let Some(port_err) = err.downcast_ref::<PortMapError>() {
        return match port_err {
            PortMapError::PcpNotSupported(_) => ExitCode::Unsupported,
            _ => ExitCode::Transient,
        };
    }

    ExitCode::Transient
}
