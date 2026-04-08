//! Service discovery file writer.
//!
//! On startup, writes `instance/data/.discovery.json` so clients (TUI, desktop)
//! can locate the server without manual URL configuration. The file contains the
//! server URL, version, start time, and optionally the Tailscale IP.
//!
//! Clients check in order: localhost, discovery file, Tailscale, manual entry.

use std::path::{Path, PathBuf};

use jiff::Timestamp;
use serde::Serialize;
use snafu::{ResultExt, Snafu};
use tracing::{info, warn};

/// Discovery file name within the instance data directory.
const DISCOVERY_FILE: &str = ".discovery.json";

/// Errors from writing the discovery file.
#[derive(Debug, Snafu)]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "snafu error variant fields are self-documenting via display format"
)]
pub enum DiscoveryError {
    /// Failed to serialize discovery info to JSON.
    #[snafu(display("failed to serialize discovery info: {source}"))]
    Serialize { source: serde_json::Error },

    /// Failed to write the discovery file to disk.
    #[snafu(display("failed to write discovery file to {}: {source}", path.display()))]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// Contents of the discovery file written to `instance/data/.discovery.json`.
#[derive(Debug, Serialize)]
pub struct DiscoveryInfo {
    /// Server URL reachable from the local machine, e.g. `http://localhost:18789`.
    pub url: String,
    /// Crate version from `Cargo.toml`.
    pub version: &'static str,
    /// ISO 8601 timestamp when the server started.
    pub started_at: String,
    /// Tailscale IP address, if the node is on a tailnet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tailscale_ip: Option<String>,
}

/// Write the discovery file to `data_dir/.discovery.json`.
///
/// Queries Tailscale for the node's IP address (best-effort; failure is logged
/// and the field is omitted). The file is written atomically via a temp file +
/// rename to avoid partial reads by clients.
///
/// # Errors
///
/// Returns [`DiscoveryError::Serialize`] if JSON serialization fails.
/// Returns [`DiscoveryError::Write`] if the file cannot be written.
pub async fn write_discovery_file(
    data_dir: &Path,
    bind_addr: &str,
) -> Result<(), DiscoveryError> {
    let tailscale_ip = query_tailscale_ip().await;

    // WHY: use the Tailscale IP in the URL when available so clients on the
    // tailnet can reach the server without knowing the LAN topology. Fall back
    // to the bind address (typically 0.0.0.0 or 127.0.0.1).
    let host = tailscale_ip
        .as_deref()
        .unwrap_or_else(|| host_from_bind(bind_addr));

    let port = port_from_bind(bind_addr);

    let info = DiscoveryInfo {
        url: format!("http://{host}:{port}"),
        version: env!("CARGO_PKG_VERSION"),
        started_at: Timestamp::now().to_string(),
        tailscale_ip,
    };

    let json = serde_json::to_string_pretty(&info).context(SerializeSnafu)?;
    let path = data_dir.join(DISCOVERY_FILE);

    // WHY: write to a temp file first, then rename, so clients never see a
    // partial file. tokio::fs::rename is atomic on the same filesystem.
    let tmp_path = data_dir.join(".discovery.json.tmp");
    tokio::fs::write(&tmp_path, json.as_bytes())
        .await
        .context(WriteSnafu {
            path: tmp_path.clone(),
        })?;
    tokio::fs::rename(&tmp_path, &path)
        .await
        .context(WriteSnafu { path: path.clone() })?;

    info!(path = %path.display(), url = %info.url, "discovery file written");
    Ok(())
}

/// Remove the discovery file on shutdown so stale info is not served.
pub async fn remove_discovery_file(data_dir: &Path) {
    let path = data_dir.join(DISCOVERY_FILE);
    if let Err(e) = tokio::fs::remove_file(&path).await {
        if e.kind() != std::io::ErrorKind::NotFound {
            warn!(path = %path.display(), error = %e, "failed to remove discovery file");
        }
    }
}

/// Query Tailscale for this node's first IPv4 address.
///
/// Runs `tailscale status --json` and extracts `Self.TailscaleIPs[0]`.
/// Returns `None` if Tailscale is not installed, not running, or the
/// output cannot be parsed.
async fn query_tailscale_ip() -> Option<String> {
    let output = tokio::process::Command::new("tailscale")
        .args(["status", "--json"])
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        warn!(status = %output.status, "tailscale status exited with error");
        return None;
    }

    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;

    // WHY: TailscaleIPs is an array; first entry is the primary IPv4 address.
    let ip = parsed
        .get("Self")?
        .get("TailscaleIPs")?
        .as_array()?
        .first()?
        .as_str()?
        .to_owned();

    info!(tailscale_ip = %ip, "discovered Tailscale IP");
    Some(ip)
}

/// Extract the host portion from a bind address like `"0.0.0.0:18789"`.
///
/// When the server binds to a wildcard address, substitute `"localhost"`
/// since `0.0.0.0` is not useful for client connections.
fn host_from_bind(bind_addr: &str) -> &str {
    let host = bind_addr
        .rsplit_once(':')
        .map_or(bind_addr, |(host, _)| host);
    match host {
        "0.0.0.0" | "::" | "[::]" => "localhost",
        _ => host,
    }
}

/// Extract the port portion from a bind address like `"0.0.0.0:18789"`.
fn port_from_bind(bind_addr: &str) -> &str {
    bind_addr.rsplit_once(':').map_or("18789", |(_, port)| port)
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn host_from_bind_wildcard_ipv4() {
        assert_eq!(host_from_bind("0.0.0.0:3000"), "localhost");
    }

    #[test]
    fn host_from_bind_specific() {
        assert_eq!(host_from_bind("192.168.0.18:18789"), "192.168.0.18");
    }

    #[test]
    fn host_from_bind_loopback() {
        assert_eq!(host_from_bind("127.0.0.1:3000"), "127.0.0.1");
    }

    #[test]
    fn port_from_bind_extracts_port() {
        assert_eq!(port_from_bind("0.0.0.0:18789"), "18789");
    }

    #[test]
    fn port_from_bind_default_when_missing() {
        assert_eq!(port_from_bind("localhost"), "18789");
    }

    #[test]
    fn discovery_info_serializes_without_tailscale() {
        let info = DiscoveryInfo {
            url: "http://localhost:18789".to_owned(),
            version: "0.13.0",
            started_at: "2026-04-06T12:00:00Z".to_owned(),
            tailscale_ip: None,
        };
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["url"], "http://localhost:18789");
        assert_eq!(json["version"], "0.13.0");
        assert!(json.get("tailscale_ip").is_none());
    }

    #[test]
    fn discovery_info_serializes_with_tailscale() {
        let info = DiscoveryInfo {
            url: "http://198.51.100.1:18789".to_owned(),
            version: "0.13.0",
            started_at: "2026-04-06T12:00:00Z".to_owned(),
            tailscale_ip: Some("198.51.100.1".to_owned()),
        };
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["tailscale_ip"], "198.51.100.1");
        assert_eq!(json["url"], "http://198.51.100.1:18789");
    }

    #[tokio::test]
    async fn write_and_remove_discovery_file() {
        let dir = tempfile::tempdir().unwrap();
        write_discovery_file(dir.path(), "0.0.0.0:18789")
            .await
            .unwrap();

        let path = dir.path().join(DISCOVERY_FILE);
        assert!(path.exists(), "discovery file should be created");

        let contents = tokio::fs::read_to_string(&path).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&contents).unwrap();
        assert!(parsed["url"].as_str().unwrap().contains("18789"));
        assert!(parsed["version"].is_string());
        assert!(parsed["started_at"].is_string());

        remove_discovery_file(dir.path()).await;
        assert!(!path.exists(), "discovery file should be removed");
    }

    #[tokio::test]
    async fn remove_nonexistent_file_succeeds_silently() {
        let dir = tempfile::tempdir().unwrap();
        // Should not panic or error - the function returns (), so we verify
        // it completes without panicking by reaching this point.
        remove_discovery_file(dir.path()).await;
        // Verify the directory still exists and is empty
        assert!(dir.path().exists());
        let entries: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
        assert!(entries.is_empty(), "directory should be empty");
    }
}
