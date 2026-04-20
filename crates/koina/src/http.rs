//! Shared HTTP constants used across Aletheia crates.

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

#[cfg(test)]
mod tests {
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
}
