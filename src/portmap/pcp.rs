use super::{PortMapRequest, PortMapping};
use crate::{
    config::PortProtocol,
    error::{PortMapError, Result},
};
use anyhow::{anyhow, Context};
use crab_nat::{self, pcp, InternetProtocol, PortMappingOptions, GATEWAY_PORT};
use std::{
    convert::TryFrom,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    num::NonZeroU16,
    time::Duration,
};
use tokio::net::UdpSocket;
use tracing::{debug, warn};

pub async fn try_map(request: &PortMapRequest) -> Result<PortMapping> {
    let internal_port = NonZeroU16::new(request.internal_port)
        .ok_or_else(|| anyhow!("internal port must be non-zero"))?;
    let client_ip = resolve_client_ip(request.gateway)
        .await
        .context("failed to determine local client address for PCP")?;
    let protocol = select_protocol(request.protocol);
    let options = mapping_options(request)?;

    debug!(
        "attempting PCP mapping via gateway {} for protocol {:?}, internal port {}",
        request.gateway, protocol, internal_port
    );

    let mapping = pcp::port_mapping(
        pcp::BaseMapRequest::new(request.gateway, client_ip, protocol, internal_port),
        None,
        None,
        options,
    )
    .await
    .map_err(|err| anyhow!(PortMapError::Pcp(err.to_string())))?;

    warn_if_protocol_mismatch(request.protocol);

    Ok(convert_mapping(mapping))
}

fn mapping_options(request: &PortMapRequest) -> Result<PortMappingOptions> {
    let external_port = request.preferred_external_port.and_then(NonZeroU16::new);
    let lifetime = if request.refresh_secs == 0 {
        None
    } else {
        Some(u32::try_from(request.refresh_secs).unwrap_or(u32::MAX))
    };

    Ok(PortMappingOptions {
        external_port,
        lifetime_seconds: lifetime,
        timeout_config: None,
    })
}

fn select_protocol(protocol: PortProtocol) -> InternetProtocol {
    match protocol {
        PortProtocol::UDP => InternetProtocol::Udp,
        _ => InternetProtocol::Tcp,
    }
}

fn warn_if_protocol_mismatch(protocol: PortProtocol) {
    if matches!(protocol, PortProtocol::BOTH) {
        warn!("PCP BOTH requested; mapping TCP only. Consider mapping UDP separately if required.");
    }
}

fn convert_mapping(mapping: crab_nat::PortMapping) -> PortMapping {
    let ttl = if mapping.lifetime() == 0 {
        None
    } else {
        Some(Duration::from_secs(u64::from(mapping.lifetime())))
    };

    PortMapping {
        external_port: mapping.external_port().get(),
        internal_port: mapping.internal_port().get(),
        ttl,
    }
}

async fn resolve_client_ip(gateway: IpAddr) -> Result<IpAddr> {
    let bind_addr = match gateway {
        IpAddr::V4(_) => SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)),
        IpAddr::V6(_) => SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0, 0, 0)),
    };

    let socket = UdpSocket::bind(bind_addr).await?;
    socket.connect((gateway, GATEWAY_PORT)).await?;
    Ok(socket.local_addr()?.ip())
}
