use crate::error::{ConfigError, Result};
use serde::Deserialize;
use std::{
    env, fs,
    path::{Path, PathBuf},
};
use tracing::debug;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub qbittorrent: QbittorrentConfig,
    pub protonvpn: ProtonVpnConfig,
    pub portmap: PortMapConfig,
    #[serde(skip)]
    source: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QbittorrentConfig {
    pub base_url: String,
    pub username: String,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub password: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProtonVpnConfig {
    #[serde(default, deserialize_with = "empty_string_as_none_path")]
    pub forwarded_port_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PortMapConfig {
    #[serde(default)]
    pub internal_port: u16,
    #[serde(default = "PortMapConfig::default_protocol")]
    pub protocol: PortProtocol,
    #[serde(default = "PortMapConfig::default_refresh_secs")]
    pub refresh_secs: u64,
    #[serde(default = "PortMapConfig::default_autodiscover")]
    pub autodiscover_gateway: bool,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub gateway: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum PortProtocol {
    TCP,
    UDP,
    BOTH,
}

impl Config {
    pub fn load(cli_path: Option<PathBuf>) -> Result<Self> {
        let path = find_config(cli_path)?;
        let raw = fs::read_to_string(&path)?;
        let mut cfg: Config = toml::from_str(&raw)?;
        cfg.source = Some(path.clone());
        cfg.post_process();
        Ok(cfg)
    }

    pub fn source_path(&self) -> Option<&Path> {
        self.source.as_deref()
    }

    pub fn qbittorrent_password(&self) -> Result<String> {
        if let Some(pass) = self
            .qbittorrent
            .password
            .as_deref()
            .filter(|p| !p.trim().is_empty())
        {
            return Ok(pass.to_string());
        }
        if let Ok(env_pass) = env::var("QB_PORT_SYNC_QB_PASSWORD") {
            if !env_pass.trim().is_empty() {
                return Ok(env_pass);
            }
        }
        Err(ConfigError::MissingQbPassword.into())
    }

    pub fn resolved_forwarded_port_path(&self) -> Option<PathBuf> {
        if let Some(path) = self.protonvpn.forwarded_port_path.clone() {
            return Some(path);
        }

        #[cfg(target_os = "linux")]
        {
            if let Some(path) = linux_default_forwarded_port_path() {
                return Some(path);
            }
        }

        None
    }

    fn post_process(&mut self) {
        if let Some(path) = self.protonvpn.forwarded_port_path.as_mut() {
            if path.is_relative() {
                if let Some(source) = self.source.as_ref().and_then(|p| p.parent()) {
                    let relative_path = path.clone();
                    *path = source.join(relative_path);
                }
            }
        }
    }
}

impl PortMapConfig {
    const fn default_protocol() -> PortProtocol {
        PortProtocol::TCP
    }

    const fn default_refresh_secs() -> u64 {
        300
    }

    const fn default_autodiscover() -> bool {
        true
    }
}

fn find_config(cli_path: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = cli_path {
        return Ok(path);
    }

    let mut candidates = Vec::new();

#[cfg(target_os = "linux")]
    {
        if let Some(base) = directories::BaseDirs::new() {
            let xdg = base.config_dir().join("qb-port-sync").join("config.toml");
            candidates.push(xdg);
        }
        candidates.push(PathBuf::from("/etc/qb-port-sync/config.toml"));
    }

    #[cfg(target_os = "macos")]
    {
        candidates.push(PathBuf::from(
            "/Library/Application Support/qb-port-sync/config.toml",
        ));
    }

    for candidate in candidates {
        if candidate.exists() {
            debug!("using configuration file at {}", candidate.display());
            return Ok(candidate);
        }
    }

    Err(ConfigError::MissingConfig.into())
}

#[cfg(target_os = "linux")]
fn linux_default_forwarded_port_path() -> Option<PathBuf> {
    if let Some(runtime_dir) = env::var_os("XDG_RUNTIME_DIR") {
        let mut path = PathBuf::from(runtime_dir);
        path.push("Proton/VPN/forwarded_port");
        return Some(path);
    }

    let uid = users::get_current_uid();
    let path = PathBuf::from(format!(
        "/run/user/{uid}/Proton/VPN/forwarded_port",
        uid = uid
    ));
    Some(path)
}

fn empty_string_as_none<'de, D>(deserializer: D) -> std::result::Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let option = Option::<String>::deserialize(deserializer)?;
    Ok(option.and_then(|s| {
        let trimmed = s.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    }))
}

fn empty_string_as_none_path<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<PathBuf>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let option = Option::<String>::deserialize(deserializer)?;
    Ok(option.and_then(|s| {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(PathBuf::from(trimmed))
        }
    }))
}
