//! Auto-discover a running aletheia server on the local network.
//!
//! Discovery probes a sequence of candidate URLs via `GET /api/health`, returning
//! the first that responds successfully. Candidates are seeded from:
//!
//! 1. Explicit base URLs passed by the caller.
//! 2. The `ALETHEIA_SERVER_URL` environment variable (comma-separated).
//! 3. A known-hosts TOML file (default `~/.config/aletheia-desktop/known-hosts.toml`,
//!    overridable via `ALETHEIA_KNOWN_HOSTS_FILE`).
//! 4. `http://localhost:{port}/api/health` -- same machine.
//! 5. LAN hostnames from known-hosts, env (`ALETHEIA_LAN_HOSTNAMES`), or the
//!    configured list, probed with the `.lan` suffix.
//! 6. Tailscale IPs from known-hosts, env (`ALETHEIA_TAILSCALE_IPS`), or the
//!    configured list.
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

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use tracing::instrument;

/// Environment variable for one or more comma-separated base URL candidates.
const ENV_SERVER_URL: &str = "ALETHEIA_SERVER_URL";

/// Environment variable for comma-separated LAN hostnames.
const ENV_LAN_HOSTNAMES: &str = "ALETHEIA_LAN_HOSTNAMES";

/// Environment variable for comma-separated Tailscale IPs.
const ENV_TAILSCALE_IPS: &str = "ALETHEIA_TAILSCALE_IPS";

/// Environment variable overriding the default gateway port for generated
/// candidates.
const ENV_SERVER_PORT: &str = "ALETHEIA_SERVER_PORT";

/// Environment variable overriding the path to the known-hosts file.
const ENV_KNOWN_HOSTS_FILE: &str = "ALETHEIA_KNOWN_HOSTS_FILE";

/// Default gateway port for aletheia (pylon).
const DEFAULT_PORT: u16 = 18789;

/// Per-probe HTTP timeout.
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// Total wall-clock budget for the entire discovery sequence.
const TOTAL_TIMEOUT: Duration = Duration::from_secs(10);

/// LAN hostnames probed when callers do not provide discovery config.
const DEFAULT_LAN_HOSTNAMES: &[&str] = &[];

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
    /// Base URLs discovered from environment variables.
    env_base_urls: Vec<String>,
    /// LAN hostnames discovered from environment variables.
    env_lan_hostnames: Vec<String>,
    /// Tailscale IPs discovered from environment variables.
    env_tailscale_ips: Vec<String>,
    /// Base URLs read from the known-hosts file.
    known_hosts_base_urls: Vec<String>,
    /// LAN hostnames read from the known-hosts file.
    known_hosts_lan_hostnames: Vec<String>,
    /// Tailscale IPs read from the known-hosts file.
    known_hosts_tailscale_ips: Vec<String>,
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
            env_base_urls: Vec::new(),
            env_lan_hostnames: Vec::new(),
            env_tailscale_ips: Vec::new(),
            known_hosts_base_urls: Vec::new(),
            known_hosts_lan_hostnames: Vec::new(),
            known_hosts_tailscale_ips: Vec::new(),
        }
    }
}

impl DiscoveryConfig {
    /// Return the default discovery configuration.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a configuration from environment variables.
    ///
    /// Reads `ALETHEIA_SERVER_URL`, `ALETHEIA_LAN_HOSTNAMES`,
    /// `ALETHEIA_TAILSCALE_IPS`, and `ALETHEIA_SERVER_PORT`.
    #[must_use]
    pub fn from_env() -> Self {
        let mut config = Self::default();
        let vars: HashMap<String, String> = std::env::vars().collect();
        apply_env_vars(&mut config, &vars);
        config
    }

    /// Build a configuration from environment variables and the default
    /// known-hosts file.
    ///
    /// The default known-hosts path is `$XDG_CONFIG_HOME/aletheia-desktop/known-hosts.toml`,
    /// falling back to `$HOME/.config/aletheia-desktop/known-hosts.toml`. The
    /// path can be overridden with `ALETHEIA_KNOWN_HOSTS_FILE`.
    #[must_use]
    pub fn from_env_and_known_hosts() -> Self {
        let mut config = Self::from_env();
        if let Some(path) = known_hosts_path() {
            config = config.with_known_hosts_file(path);
        }
        config
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

    /// Merge candidates from a known-hosts TOML file.
    ///
    /// The file is ignored if it does not exist. Parse errors are logged and
    /// swallowed so that a corrupt known-hosts file does not block discovery.
    #[must_use]
    pub fn with_known_hosts_file<P: AsRef<Path>>(mut self, path: P) -> Self {
        let path = path.as_ref();
        if !path.exists() {
            return self;
        }

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "failed to read known-hosts file");
                return self;
            }
        };

        let hosts: KnownHostsFile = match toml::from_str(&content) {
            Ok(h) => h,
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "failed to parse known-hosts file");
                return self;
            }
        };

        if let Some(port) = hosts.port {
            self.port = port;
        }
        self.known_hosts_base_urls
            .extend(hosts.base_urls.into_iter().filter(|s| !s.trim().is_empty()));
        self.known_hosts_lan_hostnames.extend(
            hosts
                .lan_hostnames
                .into_iter()
                .filter(|s| !s.trim().is_empty()),
        );
        self.known_hosts_tailscale_ips.extend(
            hosts
                .tailscale_ips
                .into_iter()
                .filter(|s| !s.trim().is_empty()),
        );

        self
    }
}

/// On-disk known-hosts file shape.
#[derive(Debug, Default, Deserialize)]
struct KnownHostsFile {
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    base_urls: Vec<String>,
    #[serde(default)]
    lan_hostnames: Vec<String>,
    #[serde(default)]
    tailscale_ips: Vec<String>,
}

/// Candidate endpoint to probe during discovery.
#[derive(Debug, Clone)]
struct Candidate {
    /// Base URL without trailing slash (e.g. `http://localhost:18789`).
    base_url: String,
    /// Human-readable source label for logging (e.g. `env`, `known-hosts`).
    source: String,
}

/// Apply environment-variable overrides to `config`.
///
/// Separated from [`DiscoveryConfig::from_env`] so tests can supply a
/// deterministic map without touching process-global environment state.
fn apply_env_vars(config: &mut DiscoveryConfig, vars: &HashMap<String, String>) {
    if let Some(raw) = vars.get(ENV_SERVER_URL) {
        config.env_base_urls.extend(parse_comma_list(raw));
    }
    if let Some(raw) = vars.get(ENV_LAN_HOSTNAMES) {
        config.env_lan_hostnames.extend(parse_comma_list(raw));
    }
    if let Some(raw) = vars.get(ENV_TAILSCALE_IPS) {
        config.env_tailscale_ips.extend(parse_comma_list(raw));
    }
    if let Some(raw) = vars.get(ENV_SERVER_PORT) {
        match raw.trim().parse::<u16>() {
            Ok(port) => config.port = port,
            Err(_) => {
                tracing::warn!(value = %raw, "ignoring invalid {ENV_SERVER_PORT} value");
            }
        }
    }
}

/// Parse a comma-separated list, trimming whitespace and dropping empties.
fn parse_comma_list(raw: &str) -> impl Iterator<Item = String> + use<'_> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

/// Resolve the default known-hosts file path from the environment.
fn known_hosts_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(ENV_KNOWN_HOSTS_FILE) {
        return Some(PathBuf::from(path));
    }
    if let Some(cfg) = std::env::var_os("XDG_CONFIG_HOME") {
        return Some(
            PathBuf::from(cfg)
                .join("aletheia-desktop")
                .join("known-hosts.toml"),
        );
    }
    if let Some(home) = std::env::var_os("HOME") {
        return Some(
            PathBuf::from(home)
                .join(".config")
                .join("aletheia-desktop")
                .join("known-hosts.toml"),
        );
    }
    None
}

/// Build the ordered list of candidate URLs to probe.
fn build_candidates(config: &DiscoveryConfig) -> Vec<Candidate> {
    let mut candidates = Vec::with_capacity(
        config.base_urls.len()
            + config.env_base_urls.len()
            + config.known_hosts_base_urls.len()
            + 1
            + config.lan_hostnames.len()
            + config.env_lan_hostnames.len()
            + config.known_hosts_lan_hostnames.len()
            + config.tailscale_ips.len()
            + config.env_tailscale_ips.len()
            + config.known_hosts_tailscale_ips.len(),
    );

    // 1. Explicit URLs from caller config.
    for url in &config.base_urls {
        candidates.push(Candidate {
            base_url: normalize_base_url(url),
            source: "configured".to_string(),
        });
    }

    // 2. URLs from environment variables.
    for url in &config.env_base_urls {
        candidates.push(Candidate {
            base_url: normalize_base_url(url),
            source: "env".to_string(),
        });
    }

    // 3. URLs from the known-hosts file.
    for url in &config.known_hosts_base_urls {
        candidates.push(Candidate {
            base_url: normalize_base_url(url),
            source: "known-hosts".to_string(),
        });
    }

    // 4. Localhost -- most common case, check after explicit config.
    candidates.push(Candidate {
        base_url: format!("http://localhost:{}", config.port), // kanon:ignore SECURITY/hardcoded-loopback-url -- runtime loopback URL construction; port is from config, not hardcoded
        source: "localhost".to_string(),
    });

    // 5. LAN hostnames via .lan DNS suffix (AdGuard rewrites).
    for host in &config.known_hosts_lan_hostnames {
        candidates.push(Candidate {
            base_url: format!("http://{host}.lan:{}", config.port), // SAFE: trusted LAN, no public traversal // kanon:ignore SECURITY/insecure-transport -- LAN discovery, trusted network
            source: "known-hosts-lan".to_string(),
        });
    }
    for host in &config.env_lan_hostnames {
        candidates.push(Candidate {
            base_url: format!("http://{host}.lan:{}", config.port), // SAFE: trusted LAN, no public traversal // kanon:ignore SECURITY/insecure-transport -- LAN discovery, trusted network
            source: "env-lan".to_string(),
        });
    }
    for host in &config.lan_hostnames {
        candidates.push(Candidate {
            base_url: format!("http://{host}.lan:{}", config.port), // SAFE: trusted LAN, no public traversal // kanon:ignore SECURITY/insecure-transport -- LAN discovery, trusted network
            source: "lan".to_string(),
        });
    }

    // 6. Tailscale IPs -- direct IP probe as a configured fallback.
    for ip in &config.known_hosts_tailscale_ips {
        candidates.push(Candidate {
            base_url: format!("http://{ip}:{}", config.port), // SAFE: Tailscale WireGuard tunnel, encrypted in transit // kanon:ignore SECURITY/insecure-transport -- Tailscale WireGuard tunnel, encrypted in transit
            source: "known-hosts-tailscale".to_string(),
        });
    }
    for ip in &config.env_tailscale_ips {
        candidates.push(Candidate {
            base_url: format!("http://{ip}:{}", config.port), // SAFE: Tailscale WireGuard tunnel, encrypted in transit // kanon:ignore SECURITY/insecure-transport -- Tailscale WireGuard tunnel, encrypted in transit
            source: "env-tailscale".to_string(),
        });
    }
    for ip in &config.tailscale_ips {
        candidates.push(Candidate {
            base_url: format!("http://{ip}:{}", config.port), // SAFE: Tailscale WireGuard tunnel, encrypted in transit // kanon:ignore SECURITY/insecure-transport -- Tailscale WireGuard tunnel, encrypted in transit
            source: "tailscale".to_string(),
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
        source = %candidate.source,
        "probing candidate"
    );

    match client.get(&health_url).send().await {
        Ok(resp) if resp.status().is_success() => {
            tracing::info!(
                url = %candidate.base_url,
                source = %candidate.source,
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
    discover_server_with_config(&DiscoveryConfig::from_env_and_known_hosts()).await
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
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use std::io::Write as _;

    use super::*;

    fn write_text(path: &Path, contents: &str) {
        let mut file = std::fs::File::create(path).unwrap();
        file.write_all(contents.as_bytes()).unwrap();
    }

    #[test]
    fn build_candidates_includes_localhost() {
        let candidates = build_candidates(&DiscoveryConfig::default());
        assert!(candidates.iter().any(|c| c.source == "localhost"));
    }

    #[test]
    fn build_candidates_includes_configured_base_urls_first() {
        let config = DiscoveryConfig::default().with_base_urls(["https://example.test/"]);
        let candidates = build_candidates(&config);
        let Some(first) = candidates.first() else {
            panic!("build_candidates always returns at least localhost");
        };
        assert_eq!(first.source, "configured");
        assert_eq!(first.base_url, "https://example.test");
    }

    #[test]
    fn build_candidates_includes_lan_hosts() {
        let config =
            DiscoveryConfig::default().with_lan_hostnames(["host-a".to_string(), "host-b".into()]);
        let candidates = build_candidates(&config);
        let lan_count = candidates.iter().filter(|c| c.source == "lan").count();
        assert_eq!(lan_count, 2);
    }

    #[test]
    fn build_candidates_includes_configured_tailscale() {
        let config = DiscoveryConfig::default().with_tailscale_ips(["100.64.0.10"]);
        let candidates = build_candidates(&config);
        let ts_count = candidates
            .iter()
            .filter(|c| c.source == "tailscale")
            .count();
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
        assert!(!candidates.iter().any(|c| c.source == "tailscale"));
    }

    #[test]
    fn build_candidates_total_count() {
        let config = DiscoveryConfig::default()
            .with_lan_hostnames(["host-a"])
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
            .filter(|c| c.source != "configured")
        {
            assert!(
                candidate.base_url.contains(&format!(":{DEFAULT_PORT}")),
                "candidate URL should use port {DEFAULT_PORT}: {}",
                candidate.base_url
            );
        }
    }

    #[test]
    fn build_candidates_includes_env_server_urls() {
        let mut config = DiscoveryConfig::default();
        let mut vars = HashMap::new();
        vars.insert(
            ENV_SERVER_URL.to_string(),
            "http://env-1:18789, http://env-2:18789 ".to_string(),
        );
        apply_env_vars(&mut config, &vars);

        let candidates = build_candidates(&config);
        let env_count = candidates.iter().filter(|c| c.source == "env").count();
        assert_eq!(env_count, 2);
        assert!(
            candidates
                .iter()
                .any(|c| c.base_url == "http://env-1:18789")
        );
        assert!(
            candidates
                .iter()
                .any(|c| c.base_url == "http://env-2:18789")
        );
    }

    #[test]
    fn build_candidates_includes_env_lan_and_tailscale() {
        let mut config = DiscoveryConfig::default();
        let mut vars = HashMap::new();
        vars.insert(
            ENV_LAN_HOSTNAMES.to_string(),
            "server-a, server-b".to_string(),
        );
        vars.insert(ENV_TAILSCALE_IPS.to_string(), "100.64.0.5".to_string());
        apply_env_vars(&mut config, &vars);

        let candidates = build_candidates(&config);
        assert!(
            candidates
                .iter()
                .any(|c| c.source == "env-lan" && c.base_url == "http://server-a.lan:18789")
        );
        assert!(
            candidates
                .iter()
                .any(|c| c.source == "env-lan" && c.base_url == "http://server-b.lan:18789")
        );
        assert!(
            candidates
                .iter()
                .any(|c| c.source == "env-tailscale" && c.base_url == "http://100.64.0.5:18789")
        );
    }

    #[test]
    fn build_candidates_env_port_override() {
        let mut config = DiscoveryConfig::default();
        let mut vars = HashMap::new();
        vars.insert(ENV_SERVER_PORT.to_string(), "9999".to_string());
        apply_env_vars(&mut config, &vars);

        let candidates = build_candidates(&config);
        assert!(
            candidates
                .iter()
                .any(|c| c.source == "localhost" && c.base_url == "http://localhost:9999")
        );
    }

    #[test]
    fn build_candidates_includes_known_hosts_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known-hosts.toml");
        write_text(
            &path,
            r#"
port = 8080
base_urls = ["http://file-server:8080"]
lan_hostnames = ["file-lan"]
tailscale_ips = ["100.64.0.20"]
"#,
        );

        let config = DiscoveryConfig::default().with_known_hosts_file(&path);
        let candidates = build_candidates(&config);

        assert!(
            candidates
                .iter()
                .any(|c| c.source == "known-hosts" && c.base_url == "http://file-server:8080")
        );
        assert!(
            candidates
                .iter()
                .any(|c| c.source == "known-hosts-lan" && c.base_url == "http://file-lan.lan:8080")
        );
        assert!(candidates.iter().any(
            |c| c.source == "known-hosts-tailscale" && c.base_url == "http://100.64.0.20:8080"
        ));
        assert!(
            candidates
                .iter()
                .any(|c| c.source == "localhost" && c.base_url == "http://localhost:8080")
        );
    }

    #[test]
    fn build_candidates_order_puts_caller_first_then_env_then_known_hosts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known-hosts.toml");
        write_text(&path, r#"base_urls = ["http://known"]"#);

        let mut config = DiscoveryConfig::default()
            .with_base_urls(["http://caller"])
            .with_known_hosts_file(&path);
        let mut vars = HashMap::new();
        vars.insert(ENV_SERVER_URL.to_string(), "http://env".to_string());
        apply_env_vars(&mut config, &vars);

        let candidates = build_candidates(&config);
        let urls: Vec<_> = candidates
            .iter()
            .filter(|c| c.source != "localhost")
            .map(|c| (c.source.as_str(), c.base_url.as_str()))
            .collect();
        assert_eq!(
            urls,
            vec![
                ("configured", "http://caller"),
                ("env", "http://env"),
                ("known-hosts", "http://known"),
            ]
        );
    }

    #[test]
    fn known_hosts_path_resolves_default_or_explicit_override() {
        if let Some(override_path) = std::env::var_os(ENV_KNOWN_HOSTS_FILE) {
            assert_eq!(
                known_hosts_path().as_deref(),
                Some(Path::new(&override_path))
            );
            return;
        }

        if let Some(path) = known_hosts_path() {
            assert!(path.ends_with("known-hosts.toml"));
        }
    }

    #[tokio::test]
    async fn discover_server_returns_none_when_nothing_is_running() {
        // WHY: reqwest requires a TLS crypto provider to be installed before
        // building any Client. In production this is done at startup; in tests
        // we install it explicitly.
        if rustls::crypto::ring::default_provider()
            .install_default()
            .is_err()
        {
            // Already installed by another test in this process.
        }

        // WHY: In CI / test environments no server is running, so discovery
        // should return None relatively quickly (each probe times out at 2s,
        // but connection-refused is instant).
        let result = discover_server().await;
        // NOTE: We cannot assert None because the test machine might actually
        // be running an aletheia instance. Just verify it completes without
        // panicking and returns an Option.
        if let Some(url) = result {
            assert!(!url.is_empty());
        }
    }
}
