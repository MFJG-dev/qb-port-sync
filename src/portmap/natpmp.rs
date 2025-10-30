use super::{PortMapRequest, PortMapping};
use crate::{
    config::PortProtocol,
    error::{PortMapError, Result},
};
use anyhow::anyhow;
use crab_nat::{self, InternetProtocol};
use std::convert::TryFrom;
use tracing::warn;

pub async fn try_map(request: &PortMapRequest) -> Result<PortMapping> {
    let protocol = select_protocol(request.protocol);
    let lifetime_seconds = Some(u32::try_from(request.refresh_secs).unwrap_or(u32::MAX));

    warn_if_protocol_mismatch(request.protocol);

    let mapping = crab_nat::natpmp::try_port_mapping(
        request.gateway,
        protocol,
        request.internal_port,
        request.preferred_external_port,
        lifetime_seconds,
    )
    .await
    .map_err(|err| anyhow!(PortMapError::NatPmp(err.to_string())))?;

    Ok(convert_mapping(mapping))
}

fn select_protocol(protocol: PortProtocol) -> InternetProtocol {
    match protocol {
        PortProtocol::UDP => InternetProtocol::Udp,
        _ => InternetProtocol::Tcp,
    }
}

fn warn_if_protocol_mismatch(protocol: PortProtocol) {
    if matches!(protocol, PortProtocol::BOTH) {
        warn!("NAT-PMP BOTH requested; mapping TCP only.");
    }
}

fn convert_mapping(mapping: crab_nat::PortMapping) -> PortMapping {
    let ttl = if mapping.lifetime.as_secs() == 0 {
        None
    } else {
        Some(mapping.lifetime)
    };

    PortMapping {
        external_port: mapping.external_port,
        internal_port: mapping.internal_port,
        ttl,
    }
}
