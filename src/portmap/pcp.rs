use super::PortMapRequest;
use crate::error::{PortMapError, Result};
use anyhow::anyhow;

pub async fn try_map(_request: &PortMapRequest) -> Result<super::PortMapping> {
    // PCP is not yet implemented in crab_nat 0.1.0
    Err(anyhow!(PortMapError::Pcp(
        "PCP not implemented in crab_nat 0.1.0".to_string()
    )))
}
