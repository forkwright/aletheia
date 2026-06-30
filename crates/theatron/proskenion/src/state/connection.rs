//! Connection state model for the desktop app's server connection.
//!
//! Tracks the lifecycle of a connection to a running `aletheia serve` (pylon)
//! instance. Components subscribe to `Signal<ConnectionState>` to render
//! connection indicators, and `Signal<ConnectionConfig>` to persist user
//! preferences.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Connection lifecycle states.
///
/// The state machine flows:
/// ```text
/// Disconnected → Connecting → Connected
///                    ↓            ↓
///                  Failed    Reconnecting(1) → Reconnecting(2) → ... → Failed
///                                  ↓
///                              Connected
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ConnectionState {
    /// No connection attempted yet, or explicitly disconnected by the user.
    #[default]
    Disconnected,
    /// Initial connection in progress (first attempt).
    Connecting,
    /// Connected: the channel is up. The connection may be healthy or
    /// degraded/unhealthy; use [`Self::is_connected`] to treat both as up.
    Connected,
    /// Connected but degraded: the channel is up yet a health or capability
    /// check reports an unhealthy status (e.g. high latency, partial outage).
    ConnectedDegraded {
        /// Human-readable degraded status description.
        status: String,
    },
    /// Lost connection, attempting to restore. `attempt` counts consecutive
    /// failures (1-indexed).
    Reconnecting {
        /// Consecutive reconnection failures (1-indexed).
        attempt: u32,
    },
    /// Connection attempt exceeded the configured timeout.
    TimedOut,
    /// Permanently failed: requires user intervention (e.g. bad URL, auth
    /// rejected, max retries exceeded).
    Failed {
        /// Human-readable failure description.
        reason: String,
    },
}

impl ConnectionState {
    /// Whether the connection is up (either healthy or degraded).
    #[cfg_attr(not(test), expect(dead_code, reason = "used in tests"))]
    #[must_use]
    pub(crate) fn is_connected(&self) -> bool {
        matches!(self, Self::Connected | Self::ConnectedDegraded { .. })
    }

    /// Whether the connection is in the [`Disconnected`](Self::Disconnected) state.
    #[cfg_attr(not(test), expect(dead_code, reason = "used in tests"))]
    #[must_use]
    pub(crate) fn is_disconnected(&self) -> bool {
        matches!(self, Self::Disconnected)
    }

    /// Human-readable label for the current connection state.
    #[cfg_attr(not(test), expect(dead_code, reason = "used in tests"))]
    #[must_use]
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::Disconnected => "disconnected",
            Self::Connecting => "connecting",
            Self::Connected => "connected",
            Self::ConnectedDegraded { .. } => "connected degraded",
            Self::Reconnecting { .. } => "reconnecting",
            Self::TimedOut => "timed out",
            Self::Failed { .. } => "failed",
        }
    }

    /// Whether the UI should show the connect form (not connected or actively
    /// trying to connect). Both healthy and degraded connected states hide the
    /// form.
    #[must_use]
    pub(crate) fn needs_connect_view(&self) -> bool {
        !matches!(self, Self::Connected | Self::ConnectedDegraded { .. })
    }
}

/// User-configurable connection parameters, persisted to disk.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionConfig {
    /// Base URL of the pylon server (e.g. `http://localhost:18789`).
    pub server_url: String,
    /// Optional authentication token. Injected as `Authorization: Bearer <token>`.
    ///
    /// Runtime-only for new writes. Deserialization accepts legacy plaintext
    /// `desktop.toml` values so `services::config` can migrate them into the
    /// desktop secret store.
    #[serde(default, skip_serializing)]
    pub auth_token: Option<String>,
    /// Whether to automatically reconnect on connection loss.
    #[serde(default = "default_auto_reconnect")]
    pub auto_reconnect: bool,
    /// Maximum time in seconds to wait for a connection attempt before timing out.
    #[serde(default = "default_connect_timeout_secs")]
    pub connect_timeout_secs: u64,
}

impl std::fmt::Debug for ConnectionConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectionConfig")
            .field("server_url", &self.server_url)
            .field(
                "auth_token",
                &self.auth_token.as_ref().map(|_| "<redacted>"),
            )
            .field("auto_reconnect", &self.auto_reconnect)
            .field("connect_timeout_secs", &self.connect_timeout_secs)
            .finish()
    }
}

fn default_connect_timeout_secs() -> u64 {
    DEFAULT_CONNECT_TIMEOUT.as_secs()
}

fn default_auto_reconnect() -> bool {
    true
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        // WHY: skene's discovery config owns the gateway port default; deriving
        // it here keeps this URL from drifting when that port changes.
        let port = skene::discovery::DiscoveryConfig::default().port;
        Self {
            server_url: format!("http://localhost:{port}"), // kanon:ignore SECURITY/hardcoded-loopback-url -- runtime loopback URL; port derived from skene discovery config, not hardcoded
            auth_token: None,
            auto_reconnect: true,
            connect_timeout_secs: DEFAULT_CONNECT_TIMEOUT.as_secs(),
        }
    }
}

/// Returns the configured connect timeout as a [`Duration`].
impl ConnectionConfig {
    /// Connection timeout as a [`Duration`].
    #[must_use]
    pub(crate) fn connect_timeout(&self) -> Duration {
        Duration::from_secs(self.connect_timeout_secs)
    }
}

/// Default timeout for an individual connection attempt.
pub(crate) const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Initial backoff delay after a connection failure.
const INITIAL_BACKOFF: Duration = Duration::from_secs(1);

/// Maximum backoff delay: caps exponential growth.
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Interval between periodic health checks once connected.
pub(crate) const HEALTH_CHECK_INTERVAL: Duration = Duration::from_secs(30);

/// Calculate backoff duration for the given attempt number (1-indexed).
///
/// Sequence: 1s, 2s, 4s, 8s, 16s, 30s, 30s, ...
#[must_use]
pub(crate) fn backoff_duration(attempt: u32) -> Duration {
    let shift = attempt.saturating_sub(1).min(63);
    let multiplier = 1u64.checked_shl(shift).unwrap_or(u64::MAX);
    let secs = INITIAL_BACKOFF.as_secs().saturating_mul(multiplier);
    Duration::from_secs(secs).min(MAX_BACKOFF)
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    #[test]
    fn connection_state_default_is_disconnected() {
        assert_eq!(ConnectionState::default(), ConnectionState::Disconnected);
    }

    #[test]
    fn connection_state_is_connected() {
        assert!(ConnectionState::Connected.is_connected());
        assert!(
            ConnectionState::ConnectedDegraded {
                status: "high latency".into()
            }
            .is_connected()
        );
        assert!(!ConnectionState::Disconnected.is_connected());
        assert!(!ConnectionState::Connecting.is_connected());
        assert!(!ConnectionState::Reconnecting { attempt: 1 }.is_connected());
        assert!(!ConnectionState::TimedOut.is_connected());
        assert!(
            !ConnectionState::Failed {
                reason: "bad".into()
            }
            .is_connected()
        );
    }

    #[test]
    fn connection_state_needs_connect_view() {
        assert!(ConnectionState::Disconnected.needs_connect_view());
        assert!(ConnectionState::Connecting.needs_connect_view());
        assert!(!ConnectionState::Connected.needs_connect_view());
        assert!(
            !ConnectionState::ConnectedDegraded {
                status: "partial outage".into()
            }
            .needs_connect_view()
        );
        assert!(ConnectionState::Reconnecting { attempt: 1 }.needs_connect_view());
        assert!(ConnectionState::TimedOut.needs_connect_view());
        assert!(
            ConnectionState::Failed {
                reason: "err".into()
            }
            .needs_connect_view()
        );
    }

    #[test]
    fn connection_state_labels() {
        assert_eq!(ConnectionState::Disconnected.label(), "disconnected");
        assert_eq!(ConnectionState::Connecting.label(), "connecting");
        assert_eq!(ConnectionState::Connected.label(), "connected");
        assert_eq!(
            ConnectionState::ConnectedDegraded {
                status: "slow".into()
            }
            .label(),
            "connected degraded"
        );
        assert_eq!(
            ConnectionState::Reconnecting { attempt: 3 }.label(),
            "reconnecting"
        );
        assert_eq!(ConnectionState::TimedOut.label(), "timed out");
        assert_eq!(
            ConnectionState::Failed { reason: "x".into() }.label(),
            "failed"
        );
    }

    #[test]
    fn backoff_sequence() {
        assert_eq!(backoff_duration(1), Duration::from_secs(1));
        assert_eq!(backoff_duration(2), Duration::from_secs(2));
        assert_eq!(backoff_duration(3), Duration::from_secs(4));
        assert_eq!(backoff_duration(4), Duration::from_secs(8));
        assert_eq!(backoff_duration(5), Duration::from_secs(16));
        assert_eq!(backoff_duration(6), Duration::from_secs(30));
        assert_eq!(backoff_duration(7), Duration::from_secs(30));
    }

    #[test]
    fn backoff_zero_attempt() {
        // attempt 0 is an edge case: should not underflow.
        let d = backoff_duration(0);
        assert!(d <= MAX_BACKOFF);
    }

    #[test]
    fn backoff_overflow_saturates() {
        // Very large attempt number should cap at MAX_BACKOFF, not panic.
        assert_eq!(backoff_duration(100), MAX_BACKOFF);
    }

    #[test]
    fn connection_config_default() {
        let cfg = ConnectionConfig::default();
        let port = skene::discovery::DiscoveryConfig::default().port;
        assert_eq!(cfg.server_url, format!("http://localhost:{port}"));
        assert!(cfg.auth_token.is_none());
        assert!(cfg.auto_reconnect);
        assert_eq!(cfg.connect_timeout_secs, 30);
    }

    #[test]
    fn connection_config_connect_timeout() {
        let cfg = ConnectionConfig {
            connect_timeout_secs: 60,
            ..ConnectionConfig::default()
        };
        assert_eq!(cfg.connect_timeout(), Duration::from_secs(60));
    }

    #[test]
    fn connection_config_serializes_without_auth_token() {
        let cfg = ConnectionConfig {
            server_url: "https://example.com:8080".to_string(),
            auth_token: Some("secret-token".to_string()),
            auto_reconnect: false,
            connect_timeout_secs: 45,
        };
        let serialized = toml::to_string(&cfg).unwrap();
        let deserialized: ConnectionConfig = toml::from_str(&serialized).unwrap();
        assert!(!serialized.contains("secret-token"));
        assert!(!serialized.contains("auth_token"));
        assert_eq!(deserialized.server_url, cfg.server_url);
        assert!(deserialized.auth_token.is_none());
        assert_eq!(deserialized.auto_reconnect, cfg.auto_reconnect);
        assert_eq!(deserialized.connect_timeout_secs, cfg.connect_timeout_secs);
    }

    #[test]
    fn connection_config_deserializes_legacy_auth_token() {
        let toml_str = r#"
server_url = "https://example.com:8080"
auth_token = "legacy-secret"
"#;
        let cfg: ConnectionConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.auth_token.as_deref(), Some("legacy-secret"));
    }

    #[test]
    fn connection_config_toml_omits_none_token() {
        let cfg = ConnectionConfig::default();
        let serialized = toml::to_string(&cfg).unwrap();
        assert!(!serialized.contains("auth_token"));
    }

    #[test]
    fn connection_config_toml_defaults_auto_reconnect() {
        let toml_str = r#"server_url = "https://example.com:8080""#;
        let cfg: ConnectionConfig = toml::from_str(toml_str).unwrap();
        assert!(cfg.auto_reconnect);
    }

    #[test]
    fn connection_config_toml_defaults_connect_timeout() {
        let toml_str = r#"server_url = "https://example.com:8080""#;
        let cfg: ConnectionConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.connect_timeout_secs, DEFAULT_CONNECT_TIMEOUT.as_secs());
    }

    #[test]
    fn connection_state_timed_out() {
        assert!(ConnectionState::TimedOut.needs_connect_view());
        assert!(!ConnectionState::TimedOut.is_connected());
        assert!(!ConnectionState::TimedOut.is_disconnected());
    }
}
