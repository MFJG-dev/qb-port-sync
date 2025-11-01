mod config;
mod error;
#[cfg(feature = "metrics")]
mod metrics_server;
mod portmap;
mod qbit;
mod report;
mod watch;

use clap::{ArgAction, Parser, ValueEnum};
use config::Config;
use error::{classify_error, ConfigError, ExitCode, Result, UnsupportedError};
use portmap::{
    map_prefer_pcp_fallback_natpmp, map_with_natpmp, map_with_pcp, Strategy as MapStrategy,
};
use qbit::{PortUpdateResult, QbitClient};
use report::JsonReport;
use reqwest::Url;
use std::path::PathBuf;
#[cfg(feature = "metrics")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(feature = "metrics")]
use std::sync::Arc;
use std::{process, time::Duration};
use tokio::{signal, sync::mpsc, time};
use tracing::{debug, error, info, warn};

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

    /// Output machine-readable JSON summary.
    #[arg(long)]
    json: bool,

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

#[derive(Debug, Clone)]
struct StrategyOutcome {
    strategy: String,
    detected_port: Option<u16>,
    verified: bool,
    note: Option<String>,
}

#[derive(Debug, Clone)]
enum StrategyPlan {
    File { path: PathBuf },
    Portmap { mode: PortmapMode },
}

#[derive(Debug, Clone, Copy)]
enum PortmapMode {
    Auto,
    PcpOnly,
    NatOnly,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    let exit_code = match run(cli).await {
        Ok((report, code, emit_json)) => {
            if emit_json {
                println!("{}", report.line().unwrap_or_else(|_| "{}".into()));
            }
            code
        }
        Err((report, err, code, emit_json)) => {
            if emit_json {
                println!("{}", report.line().unwrap_or_else(|_| "{}".into()));
            }
            error!("{err:#}");
            code
        }
    };

    process::exit(exit_code as i32);
}

async fn run(
    cli: Cli,
) -> std::result::Result<(JsonReport, ExitCode, bool), (JsonReport, anyhow::Error, ExitCode, bool)>
{
    if cli.json && !cli.once {
        let err = UnsupportedError::new("--json is only supported with --once mode");
        let mut report = JsonReport::new(strategy_opt_label(cli.strategy));
        report.applied = false;
        report.note = String::from("json mode requires --once");
        report.error = Some(err.0.clone());
        let err = anyhow::Error::from(err);
        return Err((report, err, ExitCode::Config, true));
    }

    let config = match Config::load(cli.config.clone()) {
        Ok(cfg) => cfg,
        Err(err) => {
            let code = classify_error(&err);
            let mut report = JsonReport::new(strategy_opt_label(cli.strategy));
            report.applied = false;
            report.note = String::new();
            report.error = Some(format!("{err:#}"));
            return Err((report, err, code, cli.json));
        }
    };

    let password = match config.qbittorrent_password() {
        Ok(pw) => pw,
        Err(err) => {
            let code = classify_error(&err);
            let mut report = JsonReport::new(strategy_opt_label(cli.strategy));
            report.applied = false;
            report.error = Some(format!("{err:#}"));
            return Err((report, err, code, cli.json));
        }
    };

    let base_url = match Url::parse(&config.qbittorrent.base_url) {
        Ok(url) => url,
        Err(err) => {
            let mut report = JsonReport::new(strategy_opt_label(cli.strategy));
            report.error = Some(err.to_string());
            return Err((report, err.into(), ExitCode::Config, cli.json));
        }
    };

    let client = match QbitClient::new(base_url) {
        Ok(client) => client,
        Err(err) => {
            let code = classify_error(&err);
            let mut report = JsonReport::new(strategy_opt_label(cli.strategy));
            report.error = Some(format!("{err:#}"));
            return Err((report, err, code, cli.json));
        }
    };

    if let Err(err) = client.login(&config.qbittorrent.username, &password).await {
        let code = classify_error(&err);
        let mut report = JsonReport::new(strategy_opt_label(cli.strategy));
        report.error = Some(format!("{err:#}"));
        return Err((report, err, code, cli.json));
    }

    #[cfg(feature = "metrics")]
    let health_flag = Arc::new(AtomicBool::new(false));

    #[cfg(feature = "metrics")]
    let _metrics_handle = if config.metrics.enabled && config.metrics.port > 0 {
        match metrics_server::install_recorder() {
            Ok(handle) => {
                let port = if config.health.enabled && config.health.port > 0 {
                    config.health.port
                } else {
                    config.metrics.port
                };
                let health_clone = health_flag.clone();
                tokio::spawn(async move {
                    if let Err(err) = metrics_server::run_server(port, handle, health_clone).await {
                        error!("metrics server failed: {err:#}");
                    }
                });
                Some(())
            }
            Err(err) => {
                warn!("failed to install metrics recorder: {err:#}");
                None
            }
        }
    } else if config.health.enabled && config.health.port > 0 {
        match metrics_server::install_recorder() {
            Ok(handle) => {
                let port = config.health.port;
                let health_clone = health_flag.clone();
                tokio::spawn(async move {
                    if let Err(err) = metrics_server::run_server(port, handle, health_clone).await {
                        error!("health server failed: {err:#}");
                    }
                });
                Some(())
            }
            Err(err) => {
                warn!("failed to install health recorder: {err:#}");
                None
            }
        }
    } else {
        None
    };

    let plan = match resolve_plan(cli.strategy, &config) {
        Ok(plan) => plan,
        Err(err) => {
            let code = classify_error(&err);
            let mut report = JsonReport::new(strategy_opt_label(cli.strategy));
            report.error = Some(format!("{err:#}"));
            return Err((report, err, code, cli.json));
        }
    };

    if cli.once {
        match run_once(
            plan.clone(),
            &config,
            &client,
            #[cfg(feature = "metrics")]
            health_flag.clone(),
        )
        .await
        {
            Ok(outcome) => {
                let mut report = JsonReport::new(outcome.strategy.clone());
                report.detected_port = outcome.detected_port;
                report.applied = true;
                report.verified = outcome.verified;
                report.note = outcome.note.unwrap_or_default();
                Ok((report, ExitCode::Success, cli.json))
            }
            Err(err) => {
                let code = classify_error(&err);
                let strategy_name = match &plan {
                    StrategyPlan::File { .. } => "file".to_string(),
                    StrategyPlan::Portmap { mode } => match mode {
                        PortmapMode::Auto => "auto".to_string(),
                        PortmapMode::PcpOnly => "pcp".to_string(),
                        PortmapMode::NatOnly => "natpmp".to_string(),
                    },
                };
                let mut report = JsonReport::new(strategy_name);
                report.applied = false;
                report.verified = false;
                report.note = String::new();
                report.error = Some(format!("{err:#}"));
                Err((report, err, code, cli.json))
            }
        }
    } else {
        match run_daemon(
            plan,
            &config,
            client,
            #[cfg(feature = "metrics")]
            health_flag,
        )
        .await
        {
            Ok(_) => {
                let mut report = JsonReport::new("daemon");
                report.note = String::from("exited on signal");
                Ok((report, ExitCode::Success, cli.json))
            }
            Err(err) => {
                let code = classify_error(&err);
                let mut report = JsonReport::new("daemon");
                report.error = Some(format!("{err:#}"));
                Err((report, err, code, cli.json))
            }
        }
    }
}

async fn run_once(
    plan: StrategyPlan,
    config: &Config,
    client: &QbitClient,
    #[cfg(feature = "metrics")] health_flag: Arc<AtomicBool>,
) -> Result<StrategyOutcome> {
    let bind_interface = config.bind_interface();
    match plan {
        StrategyPlan::File { path } => {
            debug!("reading forwarded port from {:?}", path);
            let port = watch::read_forwarded_port_once(config)?;
            let update = client.set_listen_port(port, bind_interface).await?;

            #[cfg(feature = "metrics")]
            {
                metrics::counter!("qb_port_sync_port_updates_total").increment(1);
                metrics::gauge!("qb_port_sync_current_port").set(update.detected_port as f64);
                metrics::gauge!("qb_port_sync_last_update_timestamp_seconds").set(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as f64,
                );
                health_flag.store(true, Ordering::Relaxed);
            }

            Ok(StrategyOutcome {
                strategy: "file".to_string(),
                detected_port: Some(update.detected_port),
                verified: update.verified,
                note: build_note(&update, None),
            })
        }
        StrategyPlan::Portmap { mode } => {
            let map_result = match mode {
                PortmapMode::Auto => map_prefer_pcp_fallback_natpmp(&config.portmap).await?,
                PortmapMode::PcpOnly => map_with_pcp(&config.portmap).await?,
                PortmapMode::NatOnly => map_with_natpmp(&config.portmap).await?,
            };
            let strategy_label = map_strategy_label(mode, map_result.strategy);
            let update = client
                .set_listen_port(map_result.external_port, bind_interface)
                .await?;

            #[cfg(feature = "metrics")]
            {
                metrics::counter!("qb_port_sync_port_updates_total").increment(1);
                metrics::gauge!("qb_port_sync_current_port").set(update.detected_port as f64);
                metrics::gauge!("qb_port_sync_last_update_timestamp_seconds").set(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as f64,
                );
                health_flag.store(true, Ordering::Relaxed);
            }

            Ok(StrategyOutcome {
                strategy: strategy_label,
                detected_port: Some(update.detected_port),
                verified: update.verified,
                note: build_note(&update, map_result.ttl),
            })
        }
    }
}

async fn run_daemon(
    plan: StrategyPlan,
    config: &Config,
    client: QbitClient,
    #[cfg(feature = "metrics")] health_flag: Arc<AtomicBool>,
) -> Result<()> {
    match plan {
        StrategyPlan::File { path } => {
            run_file_daemon(
                path,
                config,
                client,
                #[cfg(feature = "metrics")]
                health_flag,
            )
            .await
        }
        StrategyPlan::Portmap { mode } => {
            run_portmap_daemon(
                mode,
                config,
                client,
                #[cfg(feature = "metrics")]
                health_flag,
            )
            .await
        }
    }
}

async fn run_file_daemon(
    path: PathBuf,
    config: &Config,
    client: QbitClient,
    #[cfg(feature = "metrics")] health_flag: Arc<AtomicBool>,
) -> Result<()> {
    info!("starting file-watcher strategy on {:?}", path);
    let bind_interface = config.bind_interface().map(|s| s.to_string());
    let (tx, mut rx) = mpsc::channel::<u16>(16);
    let watcher_path = path.clone();
    tokio::spawn(async move {
        if let Err(err) = watch::watch_forwarded_port(watcher_path, move |port| {
            let _ = tx.try_send(port);
        })
        .await
        {
            warn!("forwarded port watcher terminated: {err:#}");
        }
    });

    loop {
        tokio::select! {
            _ = signal::ctrl_c() => {
                info!("received shutdown signal");
                return Ok(());
            }
            Some(port) = rx.recv() => {
                info!("applying forwarded port {}", port);
                match client.set_listen_port(port, bind_interface.as_deref()).await {
                    Ok(update) => {
                        #[cfg(feature = "metrics")]
                        {
                            metrics::counter!("qb_port_sync_port_updates_total").increment(1);
                            metrics::gauge!("qb_port_sync_current_port")
                                .set(update.detected_port as f64);
                            metrics::gauge!("qb_port_sync_last_update_timestamp_seconds").set(
                                std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs() as f64,
                            );
                            health_flag.store(true, Ordering::Relaxed);
                        }
                    }
                    Err(err) => {
                        warn!("failed to apply forwarded port {}: {err:#}", port);
                        #[cfg(feature = "metrics")]
                        health_flag.store(false, Ordering::Relaxed);
                    }
                }
            }
        }
    }
}

async fn run_portmap_daemon(
    mode: PortmapMode,
    config: &Config,
    client: QbitClient,
    #[cfg(feature = "metrics")] health_flag: Arc<AtomicBool>,
) -> Result<()> {
    info!("starting port-mapping strategy: {:?}", mode);
    loop {
        let next_delay = match portmap_cycle(
            &mode,
            config,
            &client,
            #[cfg(feature = "metrics")]
            &health_flag,
        )
        .await
        {
            Ok(delay) => delay,
            Err(err) => {
                warn!("port mapping cycle failed: {err:#}");
                #[cfg(feature = "metrics")]
                health_flag.store(false, Ordering::Relaxed);
                Duration::from_secs(config.portmap.refresh_secs)
            }
        };

        tokio::select! {
            _ = signal::ctrl_c() => {
                info!("received shutdown signal");
                return Ok(());
            }
            _ = time::sleep(next_delay) => {}
        }
    }
}

async fn portmap_cycle(
    mode: &PortmapMode,
    config: &Config,
    client: &QbitClient,
    #[cfg(feature = "metrics")] health_flag: &Arc<AtomicBool>,
) -> Result<Duration> {
    let bind_interface = config.bind_interface();
    let map = match mode {
        PortmapMode::Auto => map_prefer_pcp_fallback_natpmp(&config.portmap).await?,
        PortmapMode::PcpOnly => map_with_pcp(&config.portmap).await?,
        PortmapMode::NatOnly => map_with_natpmp(&config.portmap).await?,
    };

    let label = map_strategy_label(*mode, map.strategy);
    info!(
        "port mapping obtained via {}: external {}",
        label, map.external_port
    );
    let update = client
        .set_listen_port(map.external_port, bind_interface)
        .await?;

    #[cfg(feature = "metrics")]
    {
        metrics::counter!("qb_port_sync_port_updates_total").increment(1);
        metrics::gauge!("qb_port_sync_current_port").set(update.detected_port as f64);
        metrics::gauge!("qb_port_sync_last_update_timestamp_seconds").set(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as f64,
        );
        health_flag.store(update.verified, Ordering::Relaxed);
    }

    if !update.verified {
        warn!(
            "listen port verification failed after applying {}",
            map.external_port
        );
    }
    let delay = map
        .ttl
        .map(|ttl| (ttl / 2).max(Duration::from_secs(10)))
        .unwrap_or_else(|| Duration::from_secs(config.portmap.refresh_secs));
    info!("next mapping refresh in {} seconds", delay.as_secs());
    Ok(delay)
}

fn map_strategy_label(mode: PortmapMode, result_strategy: MapStrategy) -> String {
    match mode {
        PortmapMode::PcpOnly => "pcp".to_string(),
        PortmapMode::NatOnly => "natpmp".to_string(),
        PortmapMode::Auto => match result_strategy {
            MapStrategy::Pcp => "pcp".to_string(),
            MapStrategy::NatPmp => "natpmp".to_string(),
        },
    }
}

fn build_note(update: &PortUpdateResult, ttl: Option<Duration>) -> Option<String> {
    let mut notes = Vec::new();
    if let Some(ttl) = ttl {
        notes.push(format!("ttl={}s", ttl.as_secs()));
    }
    if matches!(update.random_port, Some(true)) {
        notes.push("random_port still enabled".to_string());
    }
    if matches!(update.upnp, Some(true)) {
        notes.push("upnp still enabled".to_string());
    }
    if notes.is_empty() {
        None
    } else {
        Some(notes.join("; "))
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

    #[cfg(not(target_os = "linux"))]
    let _ = config;

    false
}

fn resolve_plan(strategy: StrategyOpt, config: &Config) -> Result<StrategyPlan> {
    match strategy {
        StrategyOpt::File => {
            let path = resolve_forwarded_port_path(config)?;
            Ok(StrategyPlan::File { path })
        }
        StrategyOpt::Pcp => Ok(StrategyPlan::Portmap {
            mode: PortmapMode::PcpOnly,
        }),
        StrategyOpt::Natpmp => Ok(StrategyPlan::Portmap {
            mode: PortmapMode::NatOnly,
        }),
        StrategyOpt::Auto => {
            if prefer_file_strategy(config) {
                let path = resolve_forwarded_port_path(config)?;
                Ok(StrategyPlan::File { path })
            } else {
                Ok(StrategyPlan::Portmap {
                    mode: PortmapMode::Auto,
                })
            }
        }
    }
}

fn strategy_opt_label(opt: StrategyOpt) -> &'static str {
    match opt {
        StrategyOpt::File => "file",
        StrategyOpt::Pcp => "pcp",
        StrategyOpt::Natpmp => "natpmp",
        StrategyOpt::Auto => "auto",
    }
}

fn resolve_forwarded_port_path(config: &Config) -> Result<PathBuf> {
    let path = config.resolved_forwarded_port_path().ok_or_else(|| {
        UnsupportedError::new("forwarded port file path unavailable on this platform")
    })?;
    if let Some(parent) = path.parent() {
        if parent.exists() {
            return Ok(path);
        }
        return Err(ConfigError::ForwardedPortUnavailable(parent.display().to_string()).into());
    }
    Err(ConfigError::ForwardedPortUnavailable(path.display().to_string()).into())
}

fn init_tracing(verbose: u8) {
    let filter = match verbose {
        0 => "info",
        1 => "debug",
        _ => "trace",
    };

    #[cfg(all(target_os = "linux", feature = "journald"))]
    {
        use tracing_subscriber::layer::SubscriberExt;
        use tracing_subscriber::util::SubscriberInitExt;

        let env_filter =
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| filter.into());

        let fmt_layer = tracing_subscriber::fmt::layer().with_target(false);

        if let Ok(journald_layer) = tracing_journald::layer() {
            let _ = tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt_layer)
                .with(journald_layer)
                .try_init();
        } else {
            let _ = tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt_layer)
                .try_init();
        }
    }

    #[cfg(not(all(target_os = "linux", feature = "journald")))]
    {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| filter.into()),
            )
            .with_target(false)
            .try_init();
    }
}
