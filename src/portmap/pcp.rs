use super::{MapRequest, MapResult};
use crate::error::{PortMapError, Result};

#[cfg(feature = "pcp")]
use {
    super::{build_result, mapping_protocol, Protocol, Strategy},
    anyhow::anyhow,
    crab_nat::{pcp, InternetProtocol, PortMappingOptions},
    std::{
        net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
        num::NonZeroU16,
        time::Duration,
    },
    tokio::net::UdpSocket,
};

#[cfg(feature = "pcp")]
pub async fn map(request: MapRequest) -> Result<MapResult> {
    let protocol = mapping_protocol(request.protocol);
    let internal_port = NonZeroU16::new(request.internal_port)
        .ok_or_else(|| anyhow!("internal port must be non-zero"))?;
    let client_ip = discover_client_ip(request.gateway).await?;
    let crab_protocol = to_crab_protocol(protocol);

    let options = PortMappingOptions {
        external_port: request.external_preference.and_then(NonZeroU16::new),
        lifetime_seconds: Some(request.refresh_secs as u32),
        timeout_config: None,
    };

    match pcp::port_mapping(
        pcp::BaseMapRequest::new(request.gateway, client_ip, crab_protocol, internal_port),
        None,
        None,
        options,
    )
    .await
    {
        Ok(mapping) => {
            let ttl = to_duration(mapping.lifetime());
            Ok(build_result(
                mapping.external_port().get(),
                ttl,
                Strategy::Pcp,
            ))
        }
        Err(pcp::Failure::UnsupportedVersion(_)) => Err(PortMapError::PcpNotSupported(
            "gateway indicates PCP is unsupported".to_string(),
        )
        .into()),
        Err(err) => Err(PortMapError::Pcp(err.to_string()).into()),
    }
}

#[cfg(not(feature = "pcp"))]
#[allow(dead_code)]
pub async fn map(_request: MapRequest) -> Result<MapResult> {
    Err(PortMapError::PcpNotSupported("pcp feature not enabled at compile time".to_string()).into())
}

#[cfg(feature = "pcp")]
async fn discover_client_ip(gateway: IpAddr) -> Result<IpAddr> {
    let bind_addr = match gateway {
        IpAddr::V4(_) => SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
        IpAddr::V6(_) => SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0),
    };
    let socket = UdpSocket::bind(bind_addr).await?;
    socket.connect((gateway, crab_nat::GATEWAY_PORT)).await?;
    Ok(socket.local_addr()?.ip())
}

#[cfg(feature = "pcp")]
fn to_crab_protocol(protocol: Protocol) -> InternetProtocol {
    match protocol {
        Protocol::Tcp | Protocol::Both => InternetProtocol::Tcp,
        Protocol::Udp => InternetProtocol::Udp,
    }
}

#[cfg(feature = "pcp")]
fn to_duration(ttl_secs: u32) -> Option<Duration> {
    if ttl_secs == 0 {
        None
    } else {
        Some(Duration::from_secs(ttl_secs as u64))
    }
}
