//! Matrix channel configuration (issue #3557, Phase 2).
//!
//! Config surface for the feature-gated [`MatrixProvider`]. Owns the
//! homeserver URL, user identity, device display name, crypto-store path,
//! and the per-nous room bindings.
//!
//! [`MatrixProvider`]: agora::matrix::MatrixProvider

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Top-level Matrix channel config.
///
/// When `enabled` is `false` (default), the aletheia runtime skips loading
/// the Matrix provider even when the binary is built with `--features matrix`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct MatrixConfig {
    /// Whether the Matrix channel is active.
    pub enabled: bool,
    /// Homeserver URL (e.g. `http://menos.lan:6167`). Must include scheme.
    pub homeserver_url: String,
    /// Matrix `user_id` this deployment authenticates as (e.g. `@syn:menos.lan`).
    pub user_id: String,
    /// Device display name advertised to the homeserver during login.
    pub device_display_name: String,
    /// Path to the fjall-backed crypto store, relative to `oikos.data()`.
    ///
    /// Default: `matrix-crypto/`. The store is namespaced per agent underneath.
    pub crypto_store_path: PathBuf,
    /// Per-nous room bindings (one entry per agent on this homeserver).
    pub accounts: Vec<MatrixAccountConfig>,
}

impl Default for MatrixConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            homeserver_url: "http://127.0.0.1:6167".to_owned(),
            user_id: String::new(),
            device_display_name: "aletheia".to_owned(),
            crypto_store_path: PathBuf::from("matrix-crypto"),
            accounts: Vec::new(),
        }
    }
}

/// Per-nous Matrix binding: maps an agent id to a Matrix room.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatrixAccountConfig {
    /// Nous (agent) identifier.
    pub nous_id: String,
    /// Matrix room ID (e.g. `!abcd1234:menos.lan`).
    pub room: String,
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn default_is_disabled_with_sensible_url() {
        let c = MatrixConfig::default();
        assert!(!c.enabled);
        assert_eq!(c.homeserver_url, "http://127.0.0.1:6167");
        assert!(c.accounts.is_empty());
        assert_eq!(c.crypto_store_path, PathBuf::from("matrix-crypto"));
    }

    #[test]
    fn toml_roundtrip() {
        let original = MatrixConfig {
            enabled: true,
            homeserver_url: "http://menos.lan:6167".to_owned(),
            user_id: "@syn:menos.lan".to_owned(),
            device_display_name: "menos-syn".to_owned(),
            crypto_store_path: PathBuf::from("matrix-crypto"),
            accounts: vec![MatrixAccountConfig {
                nous_id: "syn".to_owned(),
                room: "!abcd1234:menos.lan".to_owned(),
            }],
        };

        let text = toml::to_string(&original).expect("serialize");
        let parsed: MatrixConfig = toml::from_str(&text).expect("deserialize");
        assert_eq!(parsed.enabled, original.enabled);
        assert_eq!(parsed.homeserver_url, original.homeserver_url);
        assert_eq!(parsed.user_id, original.user_id);
        assert_eq!(parsed.device_display_name, original.device_display_name);
        assert_eq!(parsed.crypto_store_path, original.crypto_store_path);
        assert_eq!(parsed.accounts.len(), 1);
        assert_eq!(parsed.accounts[0].nous_id, "syn");
        assert_eq!(parsed.accounts[0].room, "!abcd1234:menos.lan");
    }

    #[test]
    fn camel_case_serde() {
        // TOML uses camelCase keys; ensure the renamer is active.
        let c = MatrixConfig {
            enabled: true,
            homeserver_url: "http://x:1".to_owned(),
            user_id: "@a:b".to_owned(),
            device_display_name: "d".to_owned(),
            crypto_store_path: PathBuf::from("p"),
            accounts: vec![MatrixAccountConfig {
                nous_id: "n".to_owned(),
                room: "!r".to_owned(),
            }],
        };
        let text = toml::to_string(&c).expect("serialize");
        assert!(text.contains("homeserverUrl"));
        assert!(text.contains("deviceDisplayName"));
        assert!(text.contains("cryptoStorePath"));
        assert!(text.contains("nousId"));
    }
}
