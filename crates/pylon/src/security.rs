//! Security configuration for the pylon HTTP gateway.

use std::path::PathBuf;

use aletheia_taxis::config::{GatewayConfig, PerUserRateLimitConfig};

/// The insecure default CSRF token value shipped in the default config.
///
/// When this value is detected, `from_gateway` replaces it with a
/// cryptographically random per-instance token.
const INSECURE_CSRF_DEFAULT: &str = "aletheia";

/// Middleware security settings derived from gateway configuration.
#[derive(Debug, Clone)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "security flags are inherently boolean: csrf_enabled, tls_enabled, rate_limit_enabled, trust_proxy"
)]
pub struct SecurityConfig {
    /// Origins allowed by the CORS layer.
    pub allowed_origins: Vec<String>,
    /// CORS preflight cache duration.
    pub cors_max_age_secs: u64,
    /// Maximum request body size.
    pub body_limit_bytes: usize,
    /// Whether the CSRF header check is active.
    pub csrf_enabled: bool,
    /// HTTP header name for CSRF validation.
    pub csrf_header_name: String,
    /// Expected CSRF header value (per-instance CSPRNG token).
    pub csrf_header_value: String,
    /// Whether TLS termination is handled by pylon.
    pub tls_enabled: bool,
    /// Path to PEM certificate file.
    pub tls_cert_path: Option<PathBuf>,
    /// Path to PEM private key file.
    pub tls_key_path: Option<PathBuf>,
    /// Whether per-IP rate limiting is active.
    pub rate_limit_enabled: bool,
    /// Maximum requests per minute per client IP.
    pub rate_limit_requests_per_minute: u32,
    /// Trust X-Forwarded-For and X-Real-IP headers for client IP resolution.
    ///
    /// Enable only when pylon is behind a trusted reverse proxy that strips
    /// or overwrites these headers from untrusted clients. When false, the
    /// peer TCP address is used for rate limiting and logging.
    pub trust_proxy: bool,
    /// Per-user rate limiting configuration.
    pub per_user_rate_limit: PerUserRateLimitConfig,
}

/// Generate a cryptographically random 32-character hex CSRF token.
fn generate_csrf_token() -> String {
    use std::fmt::Write as _;
    let bytes: [u8; 16] = rand::random();
    let mut s = String::with_capacity(32);
    for b in &bytes {
        // NOTE: String's fmt::Write implementation is infallible.
        let _ = write!(s, "{b:02x}");
    }
    s
}

impl SecurityConfig {
    /// Build security config from the gateway configuration section.
    ///
    /// When the configured CSRF token matches the insecure shipped default,
    /// a per-instance CSPRNG token is generated to replace it.
    #[must_use]
    pub fn from_gateway(gateway: &GatewayConfig) -> Self {
        // WHY: The default token "aletheia" is published in docs and config
        // examples, making it guessable. Any deployment that hasn't set a
        // custom value gets a unique random token per server start instead.
        let csrf_header_value = if gateway.csrf.header_value == INSECURE_CSRF_DEFAULT
            || gateway.csrf.header_value.is_empty()
        {
            generate_csrf_token()
        } else {
            gateway.csrf.header_value.clone()
        };

        Self {
            allowed_origins: gateway.cors.allowed_origins.clone(),
            cors_max_age_secs: gateway.cors.max_age_secs,
            body_limit_bytes: gateway.body_limit.max_bytes,
            csrf_enabled: gateway.csrf.enabled,
            csrf_header_name: gateway.csrf.header_name.clone(),
            csrf_header_value,
            tls_enabled: gateway.tls.enabled,
            tls_cert_path: gateway.tls.cert_path.as_ref().map(PathBuf::from),
            tls_key_path: gateway.tls.key_path.as_ref().map(PathBuf::from),
            rate_limit_enabled: gateway.rate_limit.enabled,
            rate_limit_requests_per_minute: gateway.rate_limit.requests_per_minute,
            trust_proxy: false,
            per_user_rate_limit: gateway.rate_limit.per_user.clone(),
        }
    }
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            allowed_origins: Vec::new(),
            cors_max_age_secs: 3600,
            body_limit_bytes: 1_048_576,
            csrf_enabled: true,
            csrf_header_name: "x-requested-with".to_owned(),
            csrf_header_value: generate_csrf_token(),
            tls_enabled: false,
            tls_cert_path: None,
            tls_key_path: None,
            rate_limit_enabled: false,
            rate_limit_requests_per_minute: 60,
            trust_proxy: false,
            per_user_rate_limit: PerUserRateLimitConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_gateway_replaces_insecure_default_token() {
        let gateway = GatewayConfig::default();
        assert_eq!(gateway.csrf.header_value, INSECURE_CSRF_DEFAULT);
        let sec = SecurityConfig::from_gateway(&gateway);
        assert_ne!(
            sec.csrf_header_value, INSECURE_CSRF_DEFAULT,
            "insecure default must be replaced with a CSPRNG token"
        );
        assert_eq!(
            sec.csrf_header_value.len(),
            32,
            "token must be 32 hex chars"
        );
    }

    #[test]
    fn from_gateway_preserves_custom_csrf_token() {
        let mut gateway = GatewayConfig::default();
        gateway.csrf.header_value = "my-custom-token-value".to_owned();
        let sec = SecurityConfig::from_gateway(&gateway);
        assert_eq!(sec.csrf_header_value, "my-custom-token-value");
    }

    #[test]
    fn default_trust_proxy_is_false() {
        let sec = SecurityConfig::default();
        assert!(!sec.trust_proxy, "trust_proxy must default to false");
    }

    #[test]
    fn generate_csrf_token_produces_32_hex_chars() {
        let token = generate_csrf_token();
        assert_eq!(token.len(), 32);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn generate_csrf_token_is_not_static() {
        let a = generate_csrf_token();
        let b = generate_csrf_token();
        assert_ne!(
            a, b,
            "consecutive tokens must differ (collision is astronomically unlikely)"
        );
    }
}
