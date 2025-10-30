use crate::{config::Config, error::Result, qbit::QbitClient};
use anyhow::{anyhow, Context};
use notify::{Config as NotifyConfig, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    fs,
    path::Path,
    time::Duration,
};
use tokio::{sync::mpsc, time};
use tracing::{debug, info, warn};

pub fn read_forwarded_port_once(config: &Config) -> Result<u16> {
    let path = config
        .resolved_forwarded_port_path()
        .context("forwarded port path not configured")?;
    let contents = fs::read_to_string(&path)?;
    parse_port(&contents)
}

pub async fn run_file_watcher<F>(config: &Config, client: QbitClient, shutdown: F) -> Result<()>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    let path = config
        .resolved_forwarded_port_path()
        .context("forwarded port path not configured")?;

    let watch_target = if path.exists() {
        path.clone()
    } else {
        path.parent()
            .ok_or_else(|| anyhow!("forwarded port path has no parent directory"))?
            .to_path_buf()
    };

    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut watcher = create_watcher(tx)?;

    watcher.watch(&watch_target, RecursiveMode::NonRecursive)?;

    let mut last_port: Option<u16> = None;
    if let Ok(port) = read_port_async(&path).await {
        info!("initial forwarded port {}", port);
        client.update_listen_port(port).await?;
        last_port = Some(port);
    }

    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            _ = &mut shutdown => {
                info!("stopping forwarded port watcher");
                break;
            }
            Some(event) = rx.recv() => {
                match event {
                    Ok(event) => {
                        if !is_relevant(&event, &path) {
                            continue;
                        }
                        if let Some(port) = handle_event(&path).await? {
                            if last_port != Some(port) {
                                info!("forwarded port changed: {} -> {}", last_port.unwrap_or(0), port);
                                client.update_listen_port(port).await?;
                                last_port = Some(port);
                            } else {
                                debug!("forwarded port {} unchanged", port);
                            }
                        }
                    }
                    Err(err) => warn!("watcher error: {err}"),
                }
            }
            else => break,
        }
    }

    Ok(())
}

fn create_watcher(
    sender: mpsc::UnboundedSender<notify::Result<Event>>,
) -> Result<RecommendedWatcher> {
    let watcher = RecommendedWatcher::new(
        move |res: notify::Result<Event>| {
            if sender.send(res).is_err() {
                debug!("forwarded port watcher receiver dropped");
            }
        },
        NotifyConfig::default(),
    )?;
    Ok(watcher)
}

async fn handle_event(path: &Path) -> Result<Option<u16>> {
    time::sleep(Duration::from_millis(250)).await;
    match read_port_async(path).await {
        Ok(port) => Ok(Some(port)),
        Err(err) => {
            debug!("failed to read forwarded port: {err:?}");
            Ok(None)
        }
    }
}

fn is_relevant(event: &Event, watched_path: &Path) -> bool {
    if event.paths.is_empty() {
        return true;
    }
    let parent = watched_path.parent();
    event
        .paths
        .iter()
        .any(|p| p == watched_path || parent.map(|dir| p == dir).unwrap_or(false))
}

async fn read_port_async(path: &Path) -> Result<u16> {
    let contents = tokio::fs::read_to_string(path).await?;
    parse_port(&contents)
}

fn parse_port(contents: &str) -> Result<u16> {
    let trimmed = contents.trim();
    let port: u16 = trimmed
        .parse()
        .map_err(|err| anyhow!("invalid forwarded port value {trimmed:?}: {err}"))?;
    Ok(port)
}
