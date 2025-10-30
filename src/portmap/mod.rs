use crate::{
    config::{PortMapConfig, PortProtocol},
    error::{PortMapError, Result},
};
use anyhow::Context;
use std::{net::IpAddr, str::FromStr, time::Duration};
use tracing::{info, warn};

mod natpmp;
mod pcp;

#[derive(Debug, Clone)]
pub struct PortMapping {
    pub external_port: u16,
    pub internal_port: u16,
    pub ttl: Option<Duration>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MappingStrategy {
    Pcp,
    NatPmp,
}

#[derive(Debug, Clone)]
pub struct PortMapRequest {
    pub internal_port: u16,
    pub preferred_external_port: Option<u16>,
    pub protocol: PortProtocol,
    pub refresh_secs: u64,
    pub gateway: IpAddr,
}

pub async fn map_with_strategy(
    requested_strategy: MappingStrategy,
    config: &PortMapConfig,
) -> Result<(PortMapping, MappingStrategy)> {
    let request = build_request(config)?;

    match requested_strategy {
        MappingStrategy::Pcp => match pcp::try_map(&request).await {
            Ok(mapping) => {
                info!(
                    "acquired PCP port mapping for internal port {} -> external {}",
                    request.internal_port, mapping.external_port
                );
                Ok((mapping, MappingStrategy::Pcp))
            }
            Err(err) => {
                warn!("PCP mapping failed: {}; trying NAT-PMP fallback", err);
                let mapping = match natpmp::try_map(&request).await {
                    Ok(mapping) => mapping,
                    Err(nat_err) => {
                        return Err(anyhow::anyhow!(PortMapError::Exhausted)).with_context(|| {
                            format!("PCP error: {err}; NAT-PMP error: {nat_err}")
                        });
                    }
                };
                info!(
                    "acquired NAT-PMP mapping for internal port {} -> external {}",
                    request.internal_port, mapping.external_port
                );
                Ok((mapping, MappingStrategy::NatPmp))
            }
        },
        MappingStrategy::NatPmp => {
            let mapping = natpmp::try_map(&request).await?;
            info!(
                "acquired NAT-PMP mapping for internal port {} -> external {}",
                request.internal_port, mapping.external_port
            );
            Ok((mapping, MappingStrategy::NatPmp))
        }
    }
}

pub fn next_refresh_delay(mapping: &PortMapping, config: &PortMapConfig) -> Duration {
    if let Some(ttl) = mapping.ttl {
        let half = ttl / 2;
        if half.is_zero() {
            Duration::from_secs(config.refresh_secs)
        } else {
            half
        }
    } else {
        Duration::from_secs(config.refresh_secs)
    }
}

fn build_request(config: &PortMapConfig) -> Result<PortMapRequest> {
    let gateway = match (&config.gateway, config.autodiscover_gateway) {
        (Some(host), _) if !host.is_empty() => {
            IpAddr::from_str(host).map_err(|err| anyhow::anyhow!(err))?
        }
        (_, true) => autodiscover_gateway()?,
        _ => anyhow::bail!("gateway discovery disabled and no gateway provided"),
    };

    let internal_port = resolve_internal_port(config.internal_port);
    let preferred_external_port = if config.internal_port == 0 {
        Some(internal_port)
    } else {
        Some(config.internal_port)
    };

    Ok(PortMapRequest {
        internal_port,
        preferred_external_port,
        protocol: config.protocol,
        refresh_secs: config.refresh_secs,
        gateway,
    })
}

fn autodiscover_gateway() -> Result<IpAddr> {
    match default_net::get_default_gateway() {
        Ok(gateway) => Ok(gateway.ip_addr),
        Err(err) => anyhow::bail!("failed to autodiscover gateway: {err}"),
    }
}

fn resolve_internal_port(configured: u16) -> u16 {
    if configured != 0 {
        return configured;
    }

    use rand::{rngs::SmallRng, Rng, SeedableRng};

    let mut rng = SmallRng::from_entropy();
    rng.gen_range(49152..=65535)
}
