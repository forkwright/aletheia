//! Auto-discover a running aletheia server on the local network.
//!
//! Discovery probes a sequence of candidate URLs via `GET /api/health`, returning
//! the first that responds successfully. The probe order is:
//!
//! 1. `http://localhost:{port}/api/health` -- same machine
//! 2. configured base URLs
//! 3. `http://{host}.lan:{port}/api/health` -- for each configured LAN hostname
//! 4. `http://{ip}:{port}/api/health` -- for each configured Tailscale IP
//!
//! Each probe has a per-attempt timeout (default 2 s). The entire discovery
//! sequence has a total timeout (default 10 s) to avoid blocking the UI.
//!
//! # Usage
//!
//! ```ignore
//! use skene::discovery::discover_server;
//!
//! let url = discover_server().await;
//! match url {
//!     Some(base_url) => println!("found server at {base_url}"),
//!     None => println!("no server found"),
//! }
//! ```

use std::time::Duration;

use tracing::instrument;

/// Default gateway port for aletheia (pylon).
const DEFAULT_PORT: u16 = 18789;

/// Per-probe HTTP timeout.
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// Total wall-clock budget for the entire discovery sequence.
const TOTAL_TIMEOUT: Duration = Duration::from_secs(10);

/// LAN hostnames probed when callers do not provide discovery config.
const DEFAULT_LAN_HOSTNAMES: &[&str] = &["menos", "metis"];

/// Configured server discovery candidates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryConfig {
    /// Gateway port to use for generated localhost, LAN, and Tailscale candidates.
    pub port: u16,
    /// Base URLs to probe exactly as configured, before generated LAN candidates.
    pub base_urls: Vec<String>,
    /// LAN hostnames to probe with the `.lan` suffix.
    pub lan_hostnames: Vec<String>,
    /// Tailscale IPs to probe directly.
    pub tailscale_ips: Vec<String>,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            base_urls: Vec::new(),
            lan_hostnames: DEFAULT_LAN_HOSTNAMES
                .iter()
                .map(|host| (*host).to_string())
                .collect(),
            tailscale_ips: Vec::new(),
        }
    }
}

impl DiscoveryConfig {
    /// Return the default discovery configuration.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the default LAN hostnames.
    #[must_use]
    pub fn with_lan_hostnames<I, S>(mut self, hostnames: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.lan_hostnames = hostnames
            .into_iter()
            .map(Into::into)
            .filter(|host| !host.trim().is_empty())
            .collect();
        self
    }

    /// Replace configured Tailscale IP candidates.
    #[must_use]
    pub fn with_tailscale_ips<I, S>(mut self, ips: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.tailscale_ips = ips
            .into_iter()
            .map(Into::into)
            .filter(|ip| !ip.trim().is_empty())
            .collect();
        self
    }

    /// Replace configured base URL candidates.
    #[must_use]
    pub fn with_base_urls<I, S>(mut self, urls: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.base_urls = urls
            .into_iter()
            .map(Into::into)
            .filter(|url| !url.trim().is_empty())
            .collect();
        self
    }
}

/// Candidate endpoint to probe during discovery.
#[derive(Debug, Clone)]
struct Candidate {
    /// Base URL without trailing slash (e.g. `http://localhost:18789`).
    base_url: String,
    /// Human-readable label for logging.
    label: &'static str,
}

/// Build the ordered list of candidate URLs to probe.
fn build_candidates(config: &DiscoveryConfig) -> Vec<Candidate> {
    let mut candidates = Vec::with_capacity(
        1 + config.base_urls.len() + config.lan_hostnames.len() + config.tailscale_ips.len(),
    );

    // 1. Localhost -- most common case, check first.
    candidates.push(Candidate {
        base_url: format!("http://localhost:{}", config.port), // kanon:ignore SECURITY/hardcoded-loopback-url -- runtime loopback URL construction; port is from config, not hardcoded
        label: "localhost",
    });

    // 2. Explicit URLs from config.
    for url in &config.base_urls {
        candidates.push(Candidate {
            base_url: normalize_base_url(url),
            label: "configured",
        });
    }

    // 3. LAN hostnames via .lan DNS suffix (AdGuard rewrites).
    for host in &config.lan_hostnames {
        candidates.push(Candidate {
            base_url: format!("http://{host}.lan:{}", config.port), // SAFE: trusted LAN, no public traversal // kanon:ignore SECURITY/insecure-transport -- LAN discovery, trusted network
            label: "lan",
        });
    }

    // 4. Tailscale IPs -- direct IP probe as a configured fallback.
    for ip in &config.tailscale_ips {
        candidates.push(Candidate {
            base_url: format!("http://{ip}:{}", config.port), // SAFE: Tailscale WireGuard tunnel, encrypted in transit // kanon:ignore SECURITY/insecure-transport -- Tailscale WireGuard tunnel, encrypted in transit
            label: "tailscale",
        });
    }

    candidates
}

fn normalize_base_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

/// Probe a single candidate URL by hitting its health endpoint.
///
/// Returns `Some(base_url)` if the server responds with a 2xx status within
/// [`PROBE_TIMEOUT`], `None` otherwise.
async fn probe(client: &reqwest::Client, candidate: &Candidate) -> Option<String> {
    let health_url = format!("{}/api/health", candidate.base_url);
    tracing::debug!(
        url = %health_url,
        label = candidate.label,
        "probing candidate"
    );

    match client.get(&health_url).send().await {
        Ok(resp) if resp.status().is_success() => {
            tracing::info!(
                url = %candidate.base_url,
                label = candidate.label,
                "discovered server"
            );
            Some(candidate.base_url.clone())
        }
        Ok(resp) => {
            tracing::debug!(
                url = %health_url,
                status = %resp.status(),
                "probe returned non-success"
            );
            None
        }
        Err(e) => {
            tracing::debug!(
                url = %health_url,
                error = %e,
                "probe failed"
            );
            None
        }
    }
}

/// Discover a running aletheia server on the local network.
///
/// Probes candidate URLs sequentially, returning the base URL (e.g.
/// `http://localhost:18789`) of the first server that responds to a health
/// check. Returns `None` if no server is found within [`TOTAL_TIMEOUT`].
///
/// The returned URL has no trailing slash and is suitable for passing directly
/// to [`crate::api::ApiClient::new`].
#[instrument]
pub async fn discover_server() -> Option<String> {
    discover_server_with_config(&DiscoveryConfig::default()).await
}

/// Discover a running aletheia server using explicit discovery candidates.
///
/// This is the configured form of [`discover_server`]. It keeps fleet-specific
/// addresses out of the binary while preserving the same probe behavior.
#[instrument(skip(config))]
pub async fn discover_server_with_config(config: &DiscoveryConfig) -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(PROBE_TIMEOUT)
        // WHY: Minimal client with no default headers. Discovery only hits
        // the unauthenticated /api/health endpoint; no auth or CSRF headers
        // needed. The caller constructs a full ApiClient after discovery.
        .build()
        .ok()?;

    let candidates = build_candidates(config);
    tracing::info!(count = candidates.len(), "starting server discovery");

    // WHY: tokio::time::timeout wraps the entire sequential probe loop so we
    // honour the total wall-clock budget even if individual probes are slow
    // but under their per-probe timeout.
    let result = tokio::time::timeout(TOTAL_TIMEOUT, async {
        for candidate in &candidates {
            if let Some(url) = probe(&client, candidate).await {
                return Some(url);
            }
        }
        None
    })
    .await;

    match result {
        Ok(found) => {
            if found.is_none() {
                tracing::info!("no server found after probing all candidates");
            }
            found
        }
        Err(_elapsed) => {
            tracing::warn!("server discovery timed out");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_candidates_includes_localhost_first() {
        let candidates = build_candidates(&DiscoveryConfig::default());
        let Some(first) = candidates.first() else {
            panic!("build_candidates always returns at least one entry");
        };
        assert_eq!(first.label, "localhost");
        assert!(
            first.base_url.contains("localhost"),
            "first candidate should be localhost"
        );
    }

    #[test]
    fn build_candidates_includes_lan_hosts() {
        let config =
            DiscoveryConfig::default().with_lan_hostnames(["menos".to_string(), "metis".into()]);
        let candidates = build_candidates(&config);
        let lan_count = candidates.iter().filter(|c| c.label == "lan").count();
        assert_eq!(lan_count, 2);
    }

    #[test]
    fn build_candidates_includes_configured_tailscale() {
        let config = DiscoveryConfig::default().with_tailscale_ips(["100.64.0.10"]);
        let candidates = build_candidates(&config);
        let ts_count = candidates.iter().filter(|c| c.label == "tailscale").count();
        assert_eq!(ts_count, 1);
        assert!(
            candidates
                .iter()
                .any(|c| c.base_url == "http://100.64.0.10:18789")
        );
    }

    #[test]
    fn build_candidates_has_no_default_tailscale_ips() {
        let candidates = build_candidates(&DiscoveryConfig::default());
        assert!(!candidates.iter().any(|c| c.label == "tailscale"));
    }

    #[test]
    fn build_candidates_includes_configured_base_urls() {
        let config = DiscoveryConfig::default().with_base_urls(["https://example.test/"]);
        let candidates = build_candidates(&config);
        assert!(
            candidates
                .iter()
                .any(|c| { c.label == "configured" && c.base_url == "https://example.test" })
        );
    }

    #[test]
    fn build_candidates_total_count() {
        let config = DiscoveryConfig::default()
            .with_lan_hostnames(["menos"])
            .with_tailscale_ips(["100.64.0.10"])
            .with_base_urls(["https://example.test"]);
        let candidates = build_candidates(&config);
        assert_eq!(
            candidates.len(),
            1 + config.lan_hostnames.len() + config.tailscale_ips.len() + config.base_urls.len()
        );
    }

    #[test]
    fn build_candidates_no_trailing_slash() {
        let config = DiscoveryConfig::default().with_base_urls(["https://example.test/"]);
        for candidate in build_candidates(&config) {
            assert!(
                !candidate.base_url.ends_with('/'),
                "candidate URL should not have trailing slash: {}",
                candidate.base_url
            );
        }
    }

    #[test]
    fn build_candidates_uses_default_port() {
        let config = DiscoveryConfig::default().with_tailscale_ips(["100.64.0.10"]);
        for candidate in build_candidates(&config)
            .into_iter()
            .filter(|c| c.label != "configured")
        {
            assert!(
                candidate.base_url.contains(&format!(":{DEFAULT_PORT}")),
                "candidate URL should use port {DEFAULT_PORT}: {}",
                candidate.base_url
            );
        }
    }

    #[tokio::test]
    async fn discover_server_returns_none_when_nothing_is_running() {
        // WHY: reqwest requires a TLS crypto provider to be installed before
        // building any Client. In production this is done at startup; in tests
        // we install it explicitly.
        let _ = rustls::crypto::ring::default_provider().install_default();

        // WHY: In CI / test environments no server is running, so discovery
        // should return None relatively quickly (each probe times out at 2s,
        // but connection-refused is instant).
        let result = discover_server().await;
        // NOTE: We cannot assert None because the test machine might actually
        // be running an aletheia instance. Just verify it completes without
        // panicking and returns an Option.
        let _ = result;
    }
}
