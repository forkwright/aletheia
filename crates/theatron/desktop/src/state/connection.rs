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
    /// Connected and healthy: health checks passing.
    Connected,
    /// Lost connection, attempting to restore. `attempt` counts consecutive
    /// failures (1-indexed).
    Reconnecting {
        /// Consecutive reconnection failures (1-indexed).
        attempt: u32,
    },
    /// Permanently failed: requires user intervention (e.g. bad URL, auth
    /// rejected, max retries exceeded).
    Failed {
        /// Human-readable failure description.
        reason: String,
    },
}

impl ConnectionState {
    /// Whether the connection is in the `Connected` state.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected)
    }

    /// Whether the connection is in the `Disconnected` state.
    #[must_use]
    pub fn is_disconnected(&self) -> bool {
        matches!(self, Self::Disconnected)
    }

    /// Whether the UI should show the connect form (not connected or actively
    /// trying to connect).
    #[must_use]
    pub fn needs_connect_view(&self) -> bool {
        !matches!(self, Self::Connected)
    }

    /// Short label for status bar display.
    #[must_use]
    pub fn label(&self) -> &str {
        match self {
            Self::Disconnected => "disconnected",
            Self::Connecting => "connecting",
            Self::Connected => "connected",
            Self::Reconnecting { .. } => "reconnecting",
            Self::Failed { .. } => "failed",
        }
    }
}

/// User-configurable connection parameters, persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    /// Base URL of the pylon server (e.g. `http://localhost:3000`).
    pub server_url: String,
    /// Optional authentication token. Injected as `Authorization: Bearer <token>`.
    ///
    /// NOTE: Stored in plaintext in the config file for v1. Future versions
    /// should integrate with the OS keyring (e.g. libsecret on Linux,
    /// Keychain on macOS) for secure token storage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_token: Option<String>,
    /// Whether to automatically reconnect on connection loss.
    #[serde(default = "default_auto_reconnect")]
    pub auto_reconnect: bool,
}

fn default_auto_reconnect() -> bool {
    true
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            server_url: "http://localhost:3000".to_string(),
            auth_token: None,
            auto_reconnect: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Exponential backoff
// ---------------------------------------------------------------------------

/// Initial backoff delay after a connection failure.
const INITIAL_BACKOFF: Duration = Duration::from_secs(1);

/// Maximum backoff delay: caps exponential growth.
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Interval between periodic health checks once connected.
pub const HEALTH_CHECK_INTERVAL: Duration = Duration::from_secs(30);

/// Calculate backoff duration for the given attempt number (1-indexed).
///
/// Sequence: 1s, 2s, 4s, 8s, 16s, 30s, 30s, ...
#[must_use]
pub fn backoff_duration(attempt: u32) -> Duration {
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
        assert!(!ConnectionState::Disconnected.is_connected());
        assert!(!ConnectionState::Connecting.is_connected());
        assert!(!ConnectionState::Reconnecting { attempt: 1 }.is_connected());
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
        assert!(ConnectionState::Reconnecting { attempt: 1 }.needs_connect_view());
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
            ConnectionState::Reconnecting { attempt: 3 }.label(),
            "reconnecting"
        );
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
        assert_eq!(cfg.server_url, "http://localhost:3000");
        assert!(cfg.auth_token.is_none());
        assert!(cfg.auto_reconnect);
    }

    #[test]
    fn connection_config_round_trip_toml() {
        let cfg = ConnectionConfig {
            server_url: "https://example.com:8080".to_string(),
            auth_token: Some("secret-token".to_string()),
            auto_reconnect: false,
        };
        let serialized = toml::to_string(&cfg).unwrap();
        let deserialized: ConnectionConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.server_url, cfg.server_url);
        assert_eq!(deserialized.auth_token, cfg.auth_token);
        assert_eq!(deserialized.auto_reconnect, cfg.auto_reconnect);
    }

    #[test]
    fn connection_config_toml_omits_none_token() {
        let cfg = ConnectionConfig::default();
        let serialized = toml::to_string(&cfg).unwrap();
        assert!(!serialized.contains("auth_token"));
    }

    #[test]
    fn connection_config_toml_defaults_auto_reconnect() {
        let toml_str = r#"server_url = "http://localhost:3000""#;
        let cfg: ConnectionConfig = toml::from_str(toml_str).unwrap();
        assert!(cfg.auto_reconnect);
    }
}
