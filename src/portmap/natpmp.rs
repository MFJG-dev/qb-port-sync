use super::{build_result, mapping_protocol, MapRequest, MapResult, Protocol, Strategy};
use crate::error::{PortMapError, Result};
use std::{net::IpAddr, thread, time::Duration};
use tokio::task;

pub async fn map(request: MapRequest) -> Result<MapResult> {
    let protocol = mapping_protocol(request.protocol);
    let internal_port = request.internal_port;
    let external = request.external_preference.unwrap_or(0);
    let lifetime = request.refresh_secs as u32;
    let gateway = request.gateway;

    let operation = task::spawn_blocking(
        move || -> std::result::Result<(u16, Duration), PortMapError> {
            let gateway_v4 = match gateway {
                IpAddr::V4(addr) => addr,
                IpAddr::V6(_) => {
                    return Err(PortMapError::NatPmp(
                        "NAT-PMP requires an IPv4 gateway address".to_string(),
                    ));
                }
            };

            let mut client = natpmp::Natpmp::new_with(gateway_v4)
                .map_err(|err| PortMapError::NatPmp(err.to_string()))?;
            let nat_protocol = match protocol {
                Protocol::Tcp | Protocol::Both => natpmp::Protocol::TCP,
                Protocol::Udp => natpmp::Protocol::UDP,
            };
            let requested_lifetime = if lifetime == 0 { 0 } else { lifetime };
            client
                .send_port_mapping_request(
                    nat_protocol,
                    internal_port,
                    external,
                    requested_lifetime,
                )
                .map_err(|err| PortMapError::NatPmp(err.to_string()))?;

            loop {
                match client.read_response_or_retry() {
                    Ok(natpmp::Response::UDP(resp)) | Ok(natpmp::Response::TCP(resp)) => {
                        let ttl = *resp.lifetime();
                        return Ok((resp.public_port(), ttl));
                    }
                    Ok(_) => continue,
                    Err(natpmp::Error::NATPMP_TRYAGAIN) => {
                        thread::sleep(Duration::from_millis(250));
                        continue;
                    }
                    Err(err) => return Err(PortMapError::NatPmp(err.to_string())),
                }
            }
        },
    );

    let (external_port, ttl) = operation.await??;
    let ttl = if ttl.is_zero() { None } else { Some(ttl) };

    Ok(build_result(external_port, ttl, Strategy::NatPmp))
}
