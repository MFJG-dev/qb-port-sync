mod config;
mod error;
mod portmap;
mod qbit;
mod watch;

use clap::{ArgAction, Parser, ValueEnum};
use config::Config;
use error::Result;
use portmap::{MappingStrategy, PortMapping};
use qbit::QbitClient;
use reqwest::Url;
use std::path::PathBuf;
use tokio::{signal, time};
use tracing::{error, info};

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Synchronize qBittorrent listening port with ProtonVPN."
)]
struct Cli {
    /// Override configuration file path.
    #[arg(long)]
    config: Option<PathBuf>,

    /// Perform a single port sync then exit.
    #[arg(long)]
    once: bool,

    /// Select port sync strategy.
    #[arg(long, value_enum, default_value_t = StrategyOpt::Auto)]
    strategy: StrategyOpt,

    /// Increase log verbosity (-vv for debug).
    #[arg(short, long, action = ArgAction::Count)]
    verbose: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum StrategyOpt {
    File,
    Pcp,
    Natpmp,
    Auto,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EffectiveStrategy {
    File,
    Pcp,
    NatPmp,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    init_tracing(cli.verbose)?;

    let config = Config::load(cli.config.clone())?;
    let password = config.qbittorrent_password()?;
    let base_url = Url::parse(&config.qbittorrent.base_url)?;
    let client = QbitClient::new(base_url)?;

    client
        .login(&config.qbittorrent.username, &password)
        .await?;

    let effective = determine_strategy(cli.strategy, &config);

    match effective {
        EffectiveStrategy::File => info!("using file watcher strategy"),
        EffectiveStrategy::Pcp => info!("using PCP strategy"),
        EffectiveStrategy::NatPmp => info!("using NAT-PMP strategy"),
    }

    if cli.once {
        run_once(effective, &config, &client).await?;
        return Ok(());
    }

    let shutdown = async {
        signal::ctrl_c().await.ok();
        info!("received shutdown signal");
    };

    match effective {
        EffectiveStrategy::File => {
            watch::run_file_watcher(&config, client, shutdown).await?;
        }
        EffectiveStrategy::Pcp | EffectiveStrategy::NatPmp => {
            portmap_loop(&config, client, effective, shutdown).await?;
        }
    }

    Ok(())
}

fn determine_strategy(opt: StrategyOpt, config: &Config) -> EffectiveStrategy {
    match opt {
        StrategyOpt::File => EffectiveStrategy::File,
        StrategyOpt::Pcp => EffectiveStrategy::Pcp,
        StrategyOpt::Natpmp => EffectiveStrategy::NatPmp,
        StrategyOpt::Auto => {
            if prefer_file_strategy(config) {
                EffectiveStrategy::File
            } else {
                EffectiveStrategy::Pcp
            }
        }
    }
}

fn prefer_file_strategy(config: &Config) -> bool {
    #[cfg(target_os = "linux")]
    {
        if let Some(path) = config.resolved_forwarded_port_path() {
            if path.exists() {
                return true;
            }
            if let Some(parent) = path.parent() {
                return parent.exists();
            }
        }
    }

    false
}

fn init_tracing(verbose: u8) -> Result<()> {
    let filter = match verbose {
        0 => "info",
        1 => "debug",
        _ => "trace",
    };

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| filter.into()),
        )
        .with_target(false)
        .try_init()?;

    Ok(())
}

async fn run_once(strategy: EffectiveStrategy, config: &Config, client: &QbitClient) -> Result<()> {
    let port = match strategy {
        EffectiveStrategy::File => watch::read_forwarded_port_once(config)?,
        EffectiveStrategy::Pcp => {
            let (mapping, _) =
                portmap::map_with_strategy(MappingStrategy::Pcp, &config.portmap).await?;
            mapping.external_port
        }
        EffectiveStrategy::NatPmp => {
            let (mapping, _) =
                portmap::map_with_strategy(MappingStrategy::NatPmp, &config.portmap).await?;
            mapping.external_port
        }
    };

    client.update_listen_port(port).await?;

    Ok(())
}

async fn portmap_loop<F>(
    config: &Config,
    client: QbitClient,
    strategy: EffectiveStrategy,
    shutdown: F,
) -> Result<()>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    let mapping_strategy = match strategy {
        EffectiveStrategy::Pcp => MappingStrategy::Pcp,
        EffectiveStrategy::NatPmp => MappingStrategy::NatPmp,
        EffectiveStrategy::File => unreachable!("file strategy not handled here"),
    };

    tokio::pin!(shutdown);
    let mut current_strategy = mapping_strategy;

    loop {
        let (mapping, used) = tokio::select! {
            res = portmap::map_with_strategy(current_strategy, &config.portmap) => res?,
            _ = &mut shutdown => return Ok(()),
        };

        apply_mapping(&client, mapping.external_port).await?;
        current_strategy = used;

        let wait_duration = portmap::next_refresh_delay(&mapping, &config.portmap);
        tokio::select! {
            _ = &mut shutdown => return Ok(()),
            _ = time::sleep(wait_duration) => {}
        }
    }
}

async fn apply_mapping(client: &QbitClient, port: u16) -> Result<()> {
    info!("updating qBittorrent listen port to {}", port);
    client.update_listen_port(port).await
}
