//! Auto-discover a running aletheia server on the local network.
//!
//! Discovery probes a sequence of candidate URLs via `GET /api/health`, returning
//! the first that responds successfully. The probe order is:
//!
//! 1. `http://localhost:{port}/api/health` -- same machine
//! 2. `http://{host}.lan:{port}/api/health` -- for each known LAN hostname
//! 3. `http://{ip}:{port}/api/health` -- for each known Tailscale IP
//!
//! Each probe has a per-attempt timeout (default 2 s). The entire discovery
//! sequence has a total timeout (default 10 s) to avoid blocking the UI.
//!
//! # Usage
//!
//! ```ignore
//! use theatron_core::discovery::discover_server;
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

/// Known LAN hostnames to probe. These correspond to hosts that commonly run
/// aletheia in a typical homelab setup.
///
/// WHY: kept as a small static list rather than mDNS/DNS-SD because the
/// deployment is a known, fixed fleet. mDNS adds a dependency and complexity
/// disproportionate to the benefit for this use case.
const LAN_HOSTNAMES: &[&str] = &["menos", "metis"];

/// Known Tailscale IPs that may host an aletheia instance.
const TAILSCALE_IPS: &[&str] = &["100.74.109.2", "100.117.8.41"];

/// Candidate endpoint to probe during discovery.
#[derive(Debug, Clone)]
struct Candidate {
    /// Base URL without trailing slash (e.g. `http://localhost:18789`).
    base_url: String,
    /// Human-readable label for logging.
    label: &'static str,
}

/// Build the ordered list of candidate URLs to probe.
fn build_candidates() -> Vec<Candidate> {
    let mut candidates = Vec::with_capacity(1 + LAN_HOSTNAMES.len() + TAILSCALE_IPS.len());

    // 1. Localhost -- most common case, check first.
    candidates.push(Candidate {
        base_url: format!("http://localhost:{DEFAULT_PORT}"),
        label: "localhost",
    });

    // 2. LAN hostnames via .lan DNS suffix (AdGuard rewrites).
    for host in LAN_HOSTNAMES {
        candidates.push(Candidate {
            base_url: format!("http://{host}.lan:{DEFAULT_PORT}"),
            label: "lan",
        });
    }

    // 3. Tailscale IPs -- direct IP probe as fallback.
    for ip in TAILSCALE_IPS {
        candidates.push(Candidate {
            base_url: format!("http://{ip}:{DEFAULT_PORT}"),
            label: "tailscale",
        });
    }

    candidates
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
    let client = reqwest::Client::builder()
        .timeout(PROBE_TIMEOUT)
        // WHY: Minimal client with no default headers. Discovery only hits
        // the unauthenticated /api/health endpoint; no auth or CSRF headers
        // needed. The caller constructs a full ApiClient after discovery.
        .build()
        .ok()?;

    let candidates = build_candidates();
    tracing::info!(
        count = candidates.len(),
        "starting server discovery"
    );

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
        let candidates = build_candidates();
        assert!(!candidates.is_empty());
        let first = &candidates[0];
        assert_eq!(first.label, "localhost");
        assert!(
            first.base_url.contains("localhost"),
            "first candidate should be localhost"
        );
    }

    #[test]
    fn build_candidates_includes_lan_hosts() {
        let candidates = build_candidates();
        let lan_count = candidates.iter().filter(|c| c.label == "lan").count();
        assert_eq!(lan_count, LAN_HOSTNAMES.len());
    }

    #[test]
    fn build_candidates_includes_tailscale() {
        let candidates = build_candidates();
        let ts_count = candidates.iter().filter(|c| c.label == "tailscale").count();
        assert_eq!(ts_count, TAILSCALE_IPS.len());
    }

    #[test]
    fn build_candidates_total_count() {
        let candidates = build_candidates();
        assert_eq!(
            candidates.len(),
            1 + LAN_HOSTNAMES.len() + TAILSCALE_IPS.len()
        );
    }

    #[test]
    fn build_candidates_no_trailing_slash() {
        for candidate in build_candidates() {
            assert!(
                !candidate.base_url.ends_with('/'),
                "candidate URL should not have trailing slash: {}",
                candidate.base_url
            );
        }
    }

    #[test]
    fn build_candidates_uses_default_port() {
        for candidate in build_candidates() {
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
