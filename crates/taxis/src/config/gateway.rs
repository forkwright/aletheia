//! HTTP gateway configuration types.

use serde::{Deserialize, Serialize};

use koina::secret::SecretString;

/// Default time-to-live for completed turn buffers before they are reaped, in seconds.
pub const DEFAULT_TURN_BUFFER_COMPLETED_TTL_SECS: u64 = 300;

/// Maximum events retained per turn buffer to bound memory usage.
pub const DEFAULT_TURN_BUFFER_MAX_EVENTS_PER_TURN: usize = 10_000;

/// HTTP gateway configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct GatewayConfig {
    /// TCP port the gateway listens on.
    pub port: u16,
    /// Bind mode: `"localhost"` for loopback only, `"lan"` for all interfaces.
    pub bind: String,
    /// Authentication configuration.
    pub auth: GatewayAuthConfig,
    /// TLS termination settings.
    pub tls: TlsConfig,
    /// Cross-origin resource sharing policy.
    pub cors: CorsConfig,
    /// Request body size limit.
    pub body_limit: BodyLimitConfig,
    /// CSRF protection settings.
    pub csrf: CsrfConfig,
    /// Rate limiting settings.
    pub rate_limit: RateLimitConfig,
    /// SSE heartbeat interval for event subscription streams, in seconds.
    pub sse_heartbeat_interval_secs: u64,
    /// Time-to-live for completed turn buffers before they are reaped, in seconds.
    pub turn_buffer_completed_ttl_secs: u64,
    /// Maximum events retained per turn buffer to bound memory usage.
    pub turn_buffer_max_events_per_turn: usize,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            port: 18789,
            bind: "localhost".to_owned(),
            auth: GatewayAuthConfig::default(),
            tls: TlsConfig::default(),
            cors: CorsConfig::default(),
            body_limit: BodyLimitConfig::default(),
            csrf: CsrfConfig::default(),
            rate_limit: RateLimitConfig::default(),
            sse_heartbeat_interval_secs: 30,
            turn_buffer_completed_ttl_secs: DEFAULT_TURN_BUFFER_COMPLETED_TTL_SECS,
            turn_buffer_max_events_per_turn: DEFAULT_TURN_BUFFER_MAX_EVENTS_PER_TURN,
        }
    }
}

/// Gateway authentication configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct GatewayAuthConfig {
    /// Auth mode: `"token"` (bearer token), `"none"` (disabled), `"jwt"` (explicit JWT).
    pub mode: String,
    /// Role assigned to anonymous requests when auth mode is `"none"`.
    /// Valid values: `"readonly"`, `"agent"`, `"operator"`, `"admin"`. Defaults to `"admin"`.
    pub none_role: String,
    /// JWT signing key. If `None`, falls back to `ALETHEIA_JWT_SECRET` env var.
    /// Startup fails when auth mode requires JWT and this is still the default placeholder.
    ///
    /// WHY: `SecretString` prevents accidental logging of the key value.
    pub signing_key: Option<SecretString>,
    /// Explicit operator acknowledgement required to expose MCP on a non-loopback address
    /// with `auth_mode = "none"`.
    ///
    /// WHY(#5183): without authentication, the full MCP surface (sessions, memory, knowledge)
    /// is reachable from the network. This must be a deliberate, named, operator decision —
    /// not a silent fallback. Default `false` causes startup to panic on this combination.
    pub allow_unauthenticated_network_mcp: bool,
}

impl Default for GatewayAuthConfig {
    fn default() -> Self {
        Self {
            mode: "token".to_owned(),
            none_role: "admin".to_owned(),
            signing_key: None,
            allow_unauthenticated_network_mcp: false,
        }
    }
}

/// TLS termination configuration.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct TlsConfig {
    /// Whether TLS termination is active.
    pub enabled: bool,
    /// Path to the PEM-encoded certificate file.
    pub cert_path: Option<String>,
    /// Path to the PEM-encoded private key file.
    pub key_path: Option<String>,
}

/// CORS origin allowlist configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct CorsConfig {
    /// Allowed origins. Empty or `["*"]` means permissive (dev mode).
    pub allowed_origins: Vec<String>,
    /// Preflight cache duration in seconds.
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

/// Request body size limit configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct BodyLimitConfig {
    /// Maximum request body size in bytes.
    pub max_bytes: usize,
}

impl Default for BodyLimitConfig {
    fn default() -> Self {
        Self {
            max_bytes: 1_048_576,
        }
    }
}

/// CSRF protection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct CsrfConfig {
    /// Whether CSRF header checking is active.
    pub enabled: bool,
    /// Explicit acknowledgement required when CSRF protection is disabled.
    pub disable_acknowledged: bool,
    /// Required header name (e.g. `x-requested-with`).
    pub header_name: String,
    /// Required header value (e.g. `aletheia`).
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

/// Rate limiting configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct RateLimitConfig {
    /// Whether rate limiting is active.
    pub enabled: bool,
    /// Maximum requests per minute per client IP (global rate limit).
    pub requests_per_minute: u32,
    /// Trust `X-Forwarded-For` / `X-Real-IP` headers for client IP resolution.
    ///
    /// Enable only when pylon sits behind a trusted reverse proxy that strips
    /// or overwrites these headers from untrusted clients. When false, rate
    /// limits use the peer TCP address and spoofed proxy headers are ignored.
    /// Defaults to false to prevent IP spoofing bypasses.
    pub trust_proxy: bool,
    /// Per-user rate limiting settings keyed by authenticated identity.
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

/// Per-user rate limiting configuration keyed by authenticated identity.
///
/// Applies token bucket rate limiting per user with different limits for
/// general, LLM, and tool execution endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct PerUserRateLimitConfig {
    /// Whether per-user rate limiting is active.
    pub enabled: bool,
    /// Default requests per minute for general API endpoints.
    pub default_rpm: u32,
    /// Burst allowance above the sustained rate for general endpoints.
    pub default_burst: u32,
    /// Requests per minute for LLM/chat endpoints (more expensive).
    pub llm_rpm: u32,
    /// Burst allowance for LLM endpoints.
    pub llm_burst: u32,
    /// Requests per minute for tool execution endpoints.
    pub tool_rpm: u32,
    /// Burst allowance for tool execution endpoints.
    pub tool_burst: u32,
    /// Seconds after which an idle user's rate limit state is evicted.
    pub stale_after_secs: u64,
}

impl Default for PerUserRateLimitConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            default_rpm: 60,
            default_burst: 10,
            llm_rpm: 20,
            llm_burst: 5,
            tool_rpm: 30,
            tool_burst: 8,
            stale_after_secs: 600,
        }
    }
}

#[cfg(test)]
const _: () = assert!(
    DEFAULT_TURN_BUFFER_COMPLETED_TTL_SECS == pylon::turn_buffer::DEFAULT_COMPLETED_TTL.as_secs()
);
#[cfg(test)]
const _: () = assert!(
    DEFAULT_TURN_BUFFER_MAX_EVENTS_PER_TURN == pylon::turn_buffer::DEFAULT_MAX_EVENTS_PER_TURN
);
