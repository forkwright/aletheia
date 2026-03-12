//! Security configuration for the pylon HTTP gateway.

use std::path::PathBuf;

use aletheia_taxis::config::GatewayConfig;

/// Middleware security settings derived from gateway configuration.
#[derive(Debug, Clone)]
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
    /// Expected CSRF header value.
    pub csrf_header_value: String,
    /// Whether TLS termination is handled by pylon.
    pub tls_enabled: bool,
    /// Path to PEM certificate file.
    pub tls_cert_path: Option<PathBuf>,
    /// Path to PEM private key file.
    pub tls_key_path: Option<PathBuf>,
}

impl SecurityConfig {
    /// Build security config from the gateway configuration section.
    #[must_use]
    pub fn from_gateway(gateway: &GatewayConfig) -> Self {
        Self {
            allowed_origins: gateway.cors.allowed_origins.clone(),
            cors_max_age_secs: gateway.cors.max_age_secs,
            body_limit_bytes: gateway.body_limit.max_bytes,
            csrf_enabled: gateway.csrf.enabled,
            csrf_header_name: gateway.csrf.header_name.clone(),
            csrf_header_value: gateway.csrf.header_value.clone(),
            tls_enabled: gateway.tls.enabled,
            tls_cert_path: gateway.tls.cert_path.as_ref().map(PathBuf::from),
            tls_key_path: gateway.tls.key_path.as_ref().map(PathBuf::from),
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
            csrf_header_value: "aletheia".to_owned(),
            tls_enabled: false,
            tls_cert_path: None,
            tls_key_path: None,
        }
    }
}
