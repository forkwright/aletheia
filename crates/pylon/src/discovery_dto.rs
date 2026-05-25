// WHY: wire DTO
//! Discovery file wire shapes.

use serde::Serialize;

/// Contents of the discovery file written to `instance/data/.discovery.json`.
#[derive(Debug, Serialize)]
pub(crate) struct DiscoveryInfo {
    /// Server URL reachable from the local machine, e.g. `http://localhost:18789`.
    pub(crate) url: String,
    /// Crate version from `Cargo.toml`.
    pub(crate) version: &'static str,
    /// ISO 8601 timestamp when the server started.
    pub(crate) started_at: String,
    /// Tailscale IP address, if the node is on a tailnet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tailscale_ip: Option<String>,
}
