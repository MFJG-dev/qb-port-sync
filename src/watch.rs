use crate::{config::Config, error::Result};
use anyhow::Context;
use notify::{
    Config as NotifyConfig, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::{fs, sync::mpsc, time};
use tracing::{debug, warn};

pub fn read_forwarded_port_once(config: &Config) -> Result<u16> {
    let path = config
        .resolved_forwarded_port_path()
        .context("forwarded port path not configured")?;
    read_port_sync(&path)
}

pub async fn watch_forwarded_port<F>(path: PathBuf, on_change: F) -> Result<()>
where
    F: Fn(u16) + Send + 'static,
{
    let target_dir = path
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow::anyhow!("forwarded port path has no parent directory"))?;

    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut watcher = RecommendedWatcher::new(
        move |res| {
            if tx.send(res).is_err() {
                debug!("forwarded port watcher channel closed");
            }
        },
        NotifyConfig::default(),
    )?;
    watcher.watch(&target_dir, RecursiveMode::NonRecursive)?;

    let mut last_port: Option<u16> = None;
    if path.exists() {
        match read_port_sync(&path) {
            Ok(port) => {
                on_change(port);
                last_port = Some(port);
            }
            Err(err) => debug!("failed to read initial forwarded port: {err:?}"),
        }
    }

    while let Some(event) = rx.recv().await {
        match event {
            Ok(event) => {
                if !is_relevant(&event, &path) {
                    continue;
                }
                if matches!(
                    event.kind,
                    EventKind::Remove(_) | EventKind::Modify(_) | EventKind::Create(_)
                ) || event.paths.is_empty()
                {
                    if let Some(port) = handle_event(&path).await {
                        if last_port != Some(port) {
                            debug!("forwarded port file update detected: {:?}", event.kind);
                            on_change(port);
                            last_port = Some(port);
                        }
                    }
                }
            }
            Err(err) => warn!("forwarded port watcher error: {err}"),
        }
    }

    Ok(())
}

pub fn parse_port(contents: &str) -> Result<u16> {
    let trimmed = contents.trim();
    let port: u16 = trimmed
        .parse()
        .map_err(|err| anyhow::anyhow!("invalid forwarded port value {trimmed:?}: {err}"))?;
    Ok(port)
}

async fn handle_event(path: &Path) -> Option<u16> {
    time::sleep(Duration::from_millis(250)).await;
    match fs::read_to_string(path).await {
        Ok(contents) => match parse_port(&contents) {
            Ok(port) => Some(port),
            Err(err) => {
                debug!("failed to parse forwarded port contents: {err:?}");
                None
            }
        },
        Err(err) => {
            debug!("failed to read forwarded port file: {err:?}");
            None
        }
    }
}

fn is_relevant(event: &Event, watched_path: &Path) -> bool {
    if event.paths.is_empty() {
        return true;
    }
    event
        .paths
        .iter()
        .any(|candidate| candidate == watched_path || candidate.parent() == watched_path.parent())
}

fn read_port_sync(path: &Path) -> Result<u16> {
    let contents = std::fs::read_to_string(path)?;
    parse_port(&contents)
}

#[cfg(test)]
mod tests {
    use super::parse_port;

    #[test]
    fn parses_valid_ports() {
        assert_eq!(parse_port("51820").unwrap(), 51820);
    }

    #[test]
    fn rejects_invalid_ports() {
        assert!(parse_port("").is_err());
        assert!(parse_port("not-a-port").is_err());
        assert!(parse_port("70000").is_err());
    }
}
