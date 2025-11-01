use crate::{
    config::{PortMapConfig, PortProtocol},
    error::{PortMapError, Result},
};
use anyhow::{anyhow, Context};
use rand::{rngs::SmallRng, Rng, SeedableRng};
use std::{net::IpAddr, str::FromStr, time::Duration};
use tracing::{debug, info, warn};

mod natpmp;
mod pcp;

#[derive(Debug, Clone, Copy)]
pub enum Protocol {
    Tcp,
    Udp,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    #[cfg_attr(not(feature = "pcp"), allow(dead_code))]
    Pcp,
    NatPmp,
}

#[derive(Debug, Clone)]
pub struct MapResult {
    pub external_port: u16,
    pub ttl: Option<Duration>,
    pub strategy: Strategy,
}

#[derive(Debug, Clone)]
pub(crate) struct MapRequest {
    pub protocol: Protocol,
    pub gateway: IpAddr,
    pub internal_port: u16,
    pub external_preference: Option<u16>,
    pub refresh_secs: u64,
}

pub async fn map_prefer_pcp_fallback_natpmp(config: &PortMapConfig) -> Result<MapResult> {
    let request = build_request(config)?;

    match try_pcp(&request).await {
        Ok(result) => {
            info!(
                "acquired PCP mapping: internal {} -> external {}",
                request.internal_port, result.external_port
            );
            Ok(result)
        }
        Err(err) => {
            match err.downcast_ref::<PortMapError>() {
                Some(PortMapError::PcpNotSupported(_)) => {
                    debug!("PCP not supported, falling back to NAT-PMP");
                }
                Some(PortMapError::Pcp(msg)) => warn!("PCP mapping failed: {msg}"),
                _ => warn!("PCP mapping error: {err:#}"),
            }

            let result = try_natpmp(&request).await?;
            info!(
                "acquired NAT-PMP mapping: internal {} -> external {}",
                request.internal_port, result.external_port
            );
            Ok(result)
        }
    }
}

pub async fn map_with_pcp(config: &PortMapConfig) -> Result<MapResult> {
    let request = build_request(config)?;
    try_pcp(&request).await
}

pub async fn map_with_natpmp(config: &PortMapConfig) -> Result<MapResult> {
    let request = build_request(config)?;
    try_natpmp(&request).await
}

pub fn protocol_from_config(protocol: PortProtocol) -> Protocol {
    match protocol {
        PortProtocol::TCP => Protocol::Tcp,
        PortProtocol::UDP => Protocol::Udp,
        PortProtocol::BOTH => Protocol::Both,
    }
}

async fn try_pcp(request: &MapRequest) -> Result<MapResult> {
    #[cfg(feature = "pcp")]
    {
        pcp::map(request.clone()).await
    }

    #[cfg(not(feature = "pcp"))]
    {
        let _ = request;
        Err(
            PortMapError::PcpNotSupported("pcp feature not enabled at compile time".to_string())
                .into(),
        )
    }
}

async fn try_natpmp(request: &MapRequest) -> Result<MapResult> {
    natpmp::map(request.clone()).await
}

fn build_request(config: &PortMapConfig) -> Result<MapRequest> {
    let protocol = protocol_from_config(config.protocol);
    let gateway = resolve_gateway(config)?;
    let (internal_port, external_preference) = resolve_ports(config);

    Ok(MapRequest {
        protocol,
        gateway,
        internal_port,
        external_preference,
        refresh_secs: config.refresh_secs,
    })
}

fn resolve_ports(config: &PortMapConfig) -> (u16, Option<u16>) {
    if config.internal_port == 0 {
        let mut rng = SmallRng::from_entropy();
        let internal = rng.gen_range(49152..=65535);
        (internal, None)
    } else {
        (config.internal_port, Some(config.internal_port))
    }
}

fn resolve_gateway(config: &PortMapConfig) -> Result<IpAddr> {
    if let Some(ref gateway) = config.gateway {
        if !gateway.trim().is_empty() {
            return IpAddr::from_str(gateway).context("invalid configured gateway address");
        }
    }

    if config.autodiscover_gateway {
        let gateway = default_net::get_default_gateway()
            .map_err(|err| anyhow!("failed to autodiscover default gateway: {err}"))?;
        return Ok(gateway.ip_addr);
    }

    Err(anyhow!(
        "gateway discovery disabled and no gateway configured"
    ))
}

fn effective_protocol(protocol: Protocol) -> Protocol {
    match protocol {
        Protocol::Both => Protocol::Tcp,
        other => other,
    }
}

pub(crate) fn mapping_protocol(protocol: Protocol) -> Protocol {
    effective_protocol(protocol)
}

pub(crate) fn build_result(
    external_port: u16,
    ttl: Option<Duration>,
    strategy: Strategy,
) -> MapResult {
    MapResult {
        external_port,
        ttl,
        strategy,
    }
}
