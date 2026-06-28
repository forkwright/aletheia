//! Security configuration for the pylon HTTP gateway.

use std::path::PathBuf;

use koina::secret::SecretString;
use taxis::config::{GatewayConfig, PerUserRateLimitConfig};

/// CORS-specific security settings.
#[derive(Debug, Clone)]
pub struct CorsConfig {
    /// Origins allowed by the CORS layer.
    pub allowed_origins: Vec<String>,
    /// CORS preflight cache duration.
    pub max_age_secs: u64,
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            allowed_origins: Vec::new(),
            max_age_secs: 3600,
        }
    }
}

/// CSRF protection settings.
#[derive(Debug, Clone)]
pub struct CsrfConfig {
    /// Whether the CSRF header check is active.
    pub enabled: bool,
    /// Explicit acknowledgement required when CSRF protection is disabled.
    pub disable_acknowledged: bool,
    /// HTTP header name for CSRF validation.
    pub header_name: String,
    /// Expected CSRF header value.
    pub header_value: SecretString,
}

impl Default for CsrfConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            disable_acknowledged: false,
            header_name: "x-requested-with".to_owned(),
            header_value: SecretString::from("aletheia"),
        }
    }
}

/// TLS termination settings.
#[derive(Debug, Clone, Default)]
pub struct TlsConfig {
    /// Whether TLS termination is handled by pylon.
    pub enabled: bool,
    /// Path to PEM certificate file.
    pub cert_path: Option<PathBuf>,
    /// Path to PEM private key file.
    pub key_path: Option<PathBuf>,
}

/// IP-level and per-user rate limiting settings.
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Whether per-IP rate limiting is active.
    pub enabled: bool,
    /// Maximum requests per minute per client IP.
    pub requests_per_minute: u32,
    /// Trust X-Forwarded-For and X-Real-IP headers for client IP resolution.
    ///
    /// Enable only when pylon is behind a trusted reverse proxy that strips
    /// or overwrites these headers from untrusted clients. When false, the
    /// peer TCP address is used for rate limiting and logging.
    pub trust_proxy: bool,
    /// Per-user rate limiting configuration.
    pub per_user: PerUserRateLimitConfig,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            requests_per_minute: 60,
            trust_proxy: false,
            per_user: PerUserRateLimitConfig::default(),
        }
    }
}

/// Middleware security settings derived from gateway configuration.
///
/// Fields are grouped into sub-structs by concern:
/// - [`cors`](SecurityConfig::cors): CORS allowed origins and cache duration
/// - [`csrf`](SecurityConfig::csrf): header-based CSRF protection
/// - [`tls`](SecurityConfig::tls): TLS termination and certificate paths
/// - [`rate_limit`](SecurityConfig::rate_limit): IP and per-user rate limiting
#[derive(Debug, Clone)]
pub struct SecurityConfig {
    /// Maximum request body size.
    pub body_limit_bytes: usize,
    /// Cross-Origin Resource Sharing settings.
    pub cors: CorsConfig,
    /// Cross-Site Request Forgery protection settings.
    pub csrf: CsrfConfig,
    /// TLS termination settings.
    pub tls: TlsConfig,
    /// Rate limiting settings.
    pub rate_limit: RateLimitConfig,
}

impl SecurityConfig {
    /// Build security config from the gateway configuration section.
    #[must_use]
    pub fn from_gateway(gateway: &GatewayConfig) -> Self {
        Self {
            body_limit_bytes: gateway.body_limit.max_bytes,
            cors: CorsConfig {
                allowed_origins: gateway.cors.allowed_origins.clone(),
                max_age_secs: gateway.cors.max_age_secs,
            },
            csrf: CsrfConfig {
                enabled: gateway.csrf.enabled,
                disable_acknowledged: gateway.csrf.disable_acknowledged,
                header_name: gateway.csrf.header_name.clone(),
                header_value: gateway.csrf.header_value.clone(),
            },
            tls: TlsConfig {
                enabled: gateway.tls.enabled,
                cert_path: gateway.tls.cert_path.as_ref().map(PathBuf::from),
                key_path: gateway.tls.key_path.as_ref().map(PathBuf::from),
            },
            rate_limit: RateLimitConfig {
                enabled: gateway.rate_limit.enabled,
                requests_per_minute: gateway.rate_limit.requests_per_minute,
                trust_proxy: gateway.rate_limit.trust_proxy,
                per_user: gateway.rate_limit.per_user.clone(),
            },
        }
    }
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            body_limit_bytes: 1_048_576,
            cors: CorsConfig::default(),
            csrf: CsrfConfig::default(),
            tls: TlsConfig::default(),
            rate_limit: RateLimitConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_gateway_preserves_default_csrf_header_value() {
        let gateway = GatewayConfig::default();
        let sec = SecurityConfig::from_gateway(&gateway);
        assert_eq!(
            sec.csrf.header_value.expose_secret(),
            "aletheia",
            "default CSRF header value must remain stable for config compatibility"
        );
    }

    #[test]
    fn from_gateway_preserves_custom_csrf_token() {
        let mut gateway = GatewayConfig::default();
        gateway.csrf.header_value = SecretString::from("my-custom-token-value");
        let sec = SecurityConfig::from_gateway(&gateway);
        assert_eq!(
            sec.csrf.header_value.expose_secret(),
            "my-custom-token-value"
        );
    }

    #[test]
    fn default_trust_proxy_is_false() {
        let sec = SecurityConfig::default();
        assert!(
            !sec.rate_limit.trust_proxy,
            "trust_proxy must default to false"
        );
    }

    #[test]
    fn from_gateway_preserves_trust_proxy() {
        let mut gateway = GatewayConfig::default();
        gateway.rate_limit.trust_proxy = true;
        let sec = SecurityConfig::from_gateway(&gateway);
        assert!(
            sec.rate_limit.trust_proxy,
            "SecurityConfig must propagate gateway.rate_limit.trust_proxy"
        );
    }
}
