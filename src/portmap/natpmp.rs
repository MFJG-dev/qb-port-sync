use super::{PortMapRequest, PortMapping};
use crate::{
    config::PortProtocol,
    error::{PortMapError, Result},
};
use anyhow::anyhow;
use crab_nat::{self, InternetProtocol, PortMappingOptions};
use std::{convert::TryFrom, num::NonZeroU16, time::Duration};
use tracing::warn;

pub async fn try_map(request: &PortMapRequest) -> Result<PortMapping> {
    let protocol = select_protocol(request.protocol);
    let internal_port = NonZeroU16::new(request.internal_port)
        .ok_or_else(|| anyhow!("internal port must be non-zero"))?;
    let options = PortMappingOptions {
        external_port: request.preferred_external_port.and_then(NonZeroU16::new),
        lifetime_seconds: Some(u32::try_from(request.refresh_secs).unwrap_or(u32::MAX)),
        timeout_config: None,
    };

    warn_if_protocol_mismatch(request.protocol);

    let mapping = crab_nat::natpmp::port_mapping(request.gateway, protocol, internal_port, options)
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
