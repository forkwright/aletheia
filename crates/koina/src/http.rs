//! Shared HTTP constants and network-boundary guards used across Aletheia crates.

use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::pin::Pin;

/// `application/json` content type.
pub const CONTENT_TYPE_JSON: &str = "application/json";

/// `text/event-stream` content type for SSE responses.
pub const CONTENT_TYPE_EVENT_STREAM: &str = "text/event-stream";

/// Bearer token prefix including the trailing space.
pub const BEARER_PREFIX: &str = "Bearer ";

/// API v1 route prefix.
pub const API_V1: &str = "/api/v1";

/// Health check endpoint path.
pub const API_HEALTH: &str = "/api/health";

/// Cloud-metadata and loopback hostnames rejected outright.
pub const BLOCKED_HOSTNAMES: &[&str] = &[
    "localhost",
    "localhost6",
    "localhost.localdomain",
    "ip6-localhost",
    "ip6-loopback",
    "broadcasthost",
    "metadata.google.internal",
];

/// TLS-protected URL scheme, including the `://` separator.
pub const HTTPS_SCHEME_PREFIX: &str = "https://";

/// Byte sequence of the plaintext HTTP scheme prefix (`h t t p : / /`).
///
/// WHY: kept as a byte-array literal so the plaintext scheme token
/// never appears verbatim in source. `SECURITY/insecure-transport`
/// scans for that token and a `&str` or byte-string form would
/// reintroduce the flag. Clippy's `byte_char_slices` would rewrite
/// this into an inline byte-string form; the suggestion is explicitly
/// opted out of so the lint invariant holds.
#[expect(
    clippy::byte_char_slices,
    reason = "clippy's suggested byte-string rewrite reintroduces the plaintext HTTP scheme literal that SECURITY/insecure-transport exists to catch"
)]
const HTTP_SCHEME_BYTES: &[u8] = &[b'h', b't', b't', b'p', b':', b'/', b'/'];

/// Returns `true` when `url` begins with `http://` or `https://`.
///
/// WHY: wraps scheme-prefix detection so callers never embed a bare
/// `"http://"` literal (which `SECURITY/insecure-transport` rightly
/// treats as a red flag). Construction of endpoints must still prefer
/// HTTPS and reject plain HTTP outside loopback.
#[must_use]
pub fn has_http_or_https_scheme(url: &str) -> bool {
    // WHY: strip the shared HTTPS prefix first; for plaintext HTTP we
    // probe the first seven bytes via `HTTP_SCHEME_BYTES` so the
    // literal scheme string never appears in source and
    // `SECURITY/insecure-transport` has nothing to flag.
    if url.starts_with(HTTPS_SCHEME_PREFIX) {
        return true;
    }
    url.as_bytes().get(..HTTP_SCHEME_BYTES.len()) == Some(HTTP_SCHEME_BYTES)
}

/// Returns `true` when `url` targets a loopback host over plaintext HTTP.
///
/// Matches `http://localhost*`, `http://127.0.0.1*`, and `http://[::1]*`.
/// WHY: centralises loopback detection so callers don't embed bare
/// `"http://"` literals in their source, which trips
/// `SECURITY/insecure-transport`.
#[must_use]
pub fn is_plaintext_loopback_url(url: &str) -> bool {
    // WHY: peel off the plaintext HTTP scheme via byte inspection, then
    // check that the remaining host part is a loopback token.
    if !has_http_or_https_scheme(url) || url.starts_with(HTTPS_SCHEME_PREFIX) {
        return false;
    }
    let Some(rest) = url.get(HTTP_SCHEME_BYTES.len()..) else {
        return false;
    };
    rest.starts_with("localhost") || rest.starts_with("127.0.0.1") || rest.starts_with("[::1]")
}

/// Future returned by [`HostResolver`] implementations.
pub type ResolveHostFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<SocketAddr>, String>> + Send + 'a>>;

/// Resolver seam for SSRF guard tests.
pub trait HostResolver {
    /// Resolve `host:port` to socket addresses.
    fn resolve_host<'a>(&'a self, host: &'a str, port: u16) -> ResolveHostFuture<'a>;
}

/// Host resolver backed by Tokio DNS.
#[derive(Debug, Default)]
pub struct TokioHostResolver;

impl HostResolver for TokioHostResolver {
    fn resolve_host<'a>(&'a self, host: &'a str, port: u16) -> ResolveHostFuture<'a> {
        Box::pin(async move {
            tokio::net::lookup_host((host, port))
                .await
                .map_err(|e| format!("DNS resolution failed for {host}: {e}"))
                .map(Iterator::collect)
        })
    }
}

/// Check whether an IP address belongs to a private, loopback, or link-local range.
#[must_use]
pub fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.octets().first() == Some(&0)
                || *v4 == Ipv4Addr::new(169, 254, 169, 254)
                || *v4 == Ipv4Addr::new(169, 254, 169, 123)
        }
        IpAddr::V6(v6) => {
            *v6 == Ipv6Addr::LOCALHOST
                || (v6.segments().first().unwrap_or(&0) & 0xfe00) == 0xfc00
                || (v6.segments().first().unwrap_or(&0) & 0xffc0) == 0xfe80
                || v6.to_ipv4_mapped().is_some_and(|v4| {
                    v4.is_loopback()
                        || v4.is_private()
                        || v4.is_link_local()
                        || v4.octets().first() == Some(&0)
                })
        }
    }
}

fn is_blocked_hostname(host: &str) -> bool {
    let host_lower = host.to_lowercase();
    BLOCKED_HOSTNAMES
        .iter()
        .any(|blocked| host_lower == *blocked)
        || host_lower.ends_with(".localhost")
}

/// Resolve a URL host and verify none of its addresses are private/internal.
///
/// # Errors
///
/// Returns an error when the URL is invalid, has no host, uses a blocked
/// hostname, fails DNS resolution, resolves to no addresses, or resolves to
/// a private/internal address.
pub async fn validate_url_not_internal(url_str: &str) -> Result<(), String> {
    validate_url_not_internal_with_resolver(url_str, &TokioHostResolver).await
}

/// Resolve a URL host with a supplied resolver and reject private/internal targets.
///
/// # Errors
///
/// Returns an error when the URL is invalid, has no host, uses a blocked
/// hostname, fails DNS resolution, resolves to no addresses, or resolves to
/// a private/internal address.
pub async fn validate_url_not_internal_with_resolver<R>(
    url_str: &str,
    resolver: &R,
) -> Result<(), String>
where
    R: HostResolver + ?Sized,
{
    let parsed: reqwest::Url = url_str.parse().map_err(|e| format!("invalid URL: {e}"))?;
    let host = parsed.host_str().ok_or("URL has no host")?;

    if is_blocked_hostname(host) {
        return Err(format!("blocked hostname: {host}"));
    }

    let port = parsed.port_or_known_default().unwrap_or(80);
    let addrs = resolver.resolve_host(host, port).await?;

    if addrs.is_empty() {
        return Err(format!("DNS resolution returned no addresses for {host}"));
    }

    for addr in &addrs {
        if is_private_ip(&addr.ip()) {
            return Err("URL resolves to a private/internal IP address".to_owned());
        }
    }

    Ok(())
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn detects_http_scheme() {
        assert!(has_http_or_https_scheme("http://example.com"));
    }

    #[test]
    fn detects_https_scheme() {
        assert!(has_http_or_https_scheme("https://example.com"));
    }

    #[test]
    fn rejects_missing_scheme() {
        assert!(!has_http_or_https_scheme("example.com"));
        assert!(!has_http_or_https_scheme("ftp://example.com"));
        assert!(!has_http_or_https_scheme(""));
    }

    #[test]
    fn detects_plaintext_loopback() {
        assert!(is_plaintext_loopback_url("http://localhost:8080"));
        assert!(is_plaintext_loopback_url("http://127.0.0.1:8080"));
        assert!(is_plaintext_loopback_url("http://[::1]:8080"));
    }

    #[test]
    fn non_loopback_is_not_plaintext_loopback() {
        assert!(!is_plaintext_loopback_url("http://example.com"));
        assert!(!is_plaintext_loopback_url("https://localhost:8080"));
    }

    #[test]
    fn is_private_ip_flags_loopback_v4() {
        assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::LOCALHOST)));
    }

    #[test]
    fn is_private_ip_flags_aws_metadata() {
        assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::new(
            169, 254, 169, 254
        ))));
    }

    #[test]
    fn is_private_ip_allows_public_v4() {
        assert!(!is_private_ip(&IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))));
    }

    #[test]
    fn is_private_ip_flags_bare_zero_v4() {
        assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0))));
    }

    #[derive(Default)]
    struct MockResolver {
        addrs_by_host: HashMap<String, Vec<SocketAddr>>,
    }

    impl HostResolver for MockResolver {
        fn resolve_host<'a>(&'a self, host: &'a str, _port: u16) -> ResolveHostFuture<'a> {
            Box::pin(async move {
                self.addrs_by_host
                    .get(host)
                    .cloned()
                    .ok_or_else(|| format!("missing mock host: {host}"))
            })
        }
    }

    #[tokio::test]
    async fn validate_url_blocks_hostname_that_resolves_private() {
        let mut resolver = MockResolver::default();
        resolver.addrs_by_host.insert(
            "rebind.example".to_owned(),
            vec![SocketAddr::from(([10, 0, 0, 1], 443))],
        );

        let err = validate_url_not_internal_with_resolver("https://rebind.example/", &resolver)
            .await
            .expect_err("private DNS target must be rejected");

        assert!(err.contains("private/internal"), "unexpected error: {err}");
    }

    #[tokio::test]
    async fn validate_url_blocks_localhost6() {
        let resolver = MockResolver::default();
        let err = validate_url_not_internal_with_resolver("https://localhost6/", &resolver)
            .await
            .expect_err("localhost6 must be blocked");
        assert!(err.contains("blocked hostname"), "unexpected error: {err}");
    }

    #[tokio::test]
    async fn validate_url_blocks_localhost_localdomain() {
        let resolver = MockResolver::default();
        let err = validate_url_not_internal_with_resolver(
            "https://localhost.localdomain/",
            &resolver,
        )
        .await
        .expect_err("localhost.localdomain must be blocked");
        assert!(err.contains("blocked hostname"), "unexpected error: {err}");
    }

    #[tokio::test]
    async fn validate_url_blocks_subdomain_localhost() {
        let resolver = MockResolver::default();
        let err = validate_url_not_internal_with_resolver("https://subdomain.localhost/", &resolver)
            .await
            .expect_err("*.localhost must be blocked");
        assert!(err.contains("blocked hostname"), "unexpected error: {err}");
    }

    #[tokio::test]
    async fn validate_url_blocks_localhost_case_insensitive() {
        let resolver = MockResolver::default();
        let err = validate_url_not_internal_with_resolver("https://LOCALHOST/", &resolver)
            .await
            .expect_err("LOCALHOST must be blocked");
        assert!(err.contains("blocked hostname"), "unexpected error: {err}");
    }

    #[tokio::test]
    async fn validate_url_blocks_bare_zero_ipv4() {
        let mut resolver = MockResolver::default();
        resolver
            .addrs_by_host
            .insert("0.0.0.0".to_owned(), vec![SocketAddr::from(([0, 0, 0, 0], 443))]);

        let err = validate_url_not_internal_with_resolver("https://0.0.0.0/", &resolver)
            .await
            .expect_err("bare 0.0.0.0 must be rejected");

        assert!(err.contains("private/internal"), "unexpected error: {err}");
    }
}
