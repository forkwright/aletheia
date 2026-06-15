//! Config redaction: strips secrets from config before API exposure.

use serde_json::Value;
use tracing::debug;

use koina::secret::SecretString;

use crate::config::AletheiaConfig;
use crate::sensitive::key_is_sensitive;

const REDACTED: &str = "***";
const SECRET_REDACTED: &str = "[REDACTED]";

type SecretAccessor = for<'a> fn(&'a AletheiaConfig) -> Option<&'a SecretString>;
type RequiredSecretAccessor = for<'a> fn(&'a AletheiaConfig) -> &'a SecretString;

#[derive(Clone, Copy)]
enum SensitiveLeafValue {
    Secret(SecretAccessor),
    RequiredSecret(RequiredSecretAccessor),
    RedactOnly,
}

#[derive(Clone, Copy)]
struct SensitiveLeaf {
    path: &'static [&'static str],
    value: SensitiveLeafValue,
}

/// Paths whose leaf values are replaced with `"***"` in redacted output.
const SENSITIVE_LEAVES: &[SensitiveLeaf] = &[
    SensitiveLeaf {
        path: &["gateway", "auth", "signingKey"],
        value: SensitiveLeafValue::Secret(gateway_auth_signing_key),
    },
    SensitiveLeaf {
        path: &["gateway", "csrf", "headerValue"],
        value: SensitiveLeafValue::RequiredSecret(gateway_csrf_header_value),
    },
    SensitiveLeaf {
        path: &["gateway", "tls", "keyPath"],
        value: SensitiveLeafValue::RedactOnly,
    },
    SensitiveLeaf {
        path: &["gateway", "tls", "certPath"],
        value: SensitiveLeafValue::RedactOnly,
    },
];

/// Serialize config to JSON, then redact sensitive fields.
#[must_use]
pub fn redact(config: &AletheiaConfig) -> Value {
    let mut value = serde_json::to_value(config).unwrap_or_else(|e| {
        debug!(error = %e, "failed to serialize config for redaction");
        Value::Null
    });
    redact_sensitive_leaves(&mut value);
    redact_sensitive_keys(&mut value);
    value
}

fn redact_sensitive_leaves(root: &mut Value) {
    for leaf in SENSITIVE_LEAVES {
        let json_pointer = format!("/{}", leaf.path.join("/"));
        if let Some(val) = root.pointer_mut(&json_pointer)
            && (val.is_string() || val.is_null())
        {
            *val = Value::String(REDACTED.to_owned());
        }
    }
}

/// Restore redacted secret leaves from the current in-memory config.
///
/// Call this after serializing `AletheiaConfig` through serde for mutation,
/// before deserializing the value back into typed config.
pub fn preserve_secret_leaves(root: &mut Value, current: &AletheiaConfig) {
    for leaf in SENSITIVE_LEAVES {
        let secret = match leaf.value {
            SensitiveLeafValue::Secret(accessor) => accessor(current),
            SensitiveLeafValue::RequiredSecret(accessor) => Some(accessor(current)),
            SensitiveLeafValue::RedactOnly => continue,
        };
        let Some(secret) = secret else {
            continue;
        };
        let Some(slot) = json_path_mut(root, leaf.path) else {
            continue;
        };
        if is_redaction_marker(slot) {
            *slot = Value::String(secret.expose_secret().to_owned());
        }
    }
}

/// Return `staged` with any redacted secret leaves restored from `current`.
///
/// This is for config paths that must serialize through `serde_json::Value`
/// before producing a live `AletheiaConfig`.
///
/// # Errors
///
/// Returns an error if the restored JSON cannot deserialize into config.
pub fn preserve_config_secret_leaves(
    staged: &AletheiaConfig,
    current: &AletheiaConfig,
) -> Result<AletheiaConfig, serde_json::Error> {
    let mut value = serde_json::to_value(staged)?;
    preserve_secret_leaves(&mut value, current);
    serde_json::from_value(value)
}

pub(crate) fn expose_secret_leaves_for_toml(root: &mut toml::Value, current: &AletheiaConfig) {
    for leaf in SENSITIVE_LEAVES {
        let secret = match leaf.value {
            SensitiveLeafValue::Secret(accessor) => accessor(current),
            SensitiveLeafValue::RequiredSecret(accessor) => Some(accessor(current)),
            SensitiveLeafValue::RedactOnly => continue,
        };
        let Some(secret) = secret else {
            continue;
        };
        let Some(slot) = toml_path_mut(root, leaf.path) else {
            continue;
        };
        if slot.as_str().is_some_and(is_redaction_marker_str) {
            *slot = toml::Value::String(secret.expose_secret().to_owned());
        }
    }
}

fn gateway_auth_signing_key(config: &AletheiaConfig) -> Option<&SecretString> {
    config.gateway.auth.signing_key.as_ref()
}

fn gateway_csrf_header_value(config: &AletheiaConfig) -> &SecretString {
    &config.gateway.csrf.header_value
}

fn is_redaction_marker(value: &Value) -> bool {
    value.as_str().is_some_and(is_redaction_marker_str)
}

fn is_redaction_marker_str(value: &str) -> bool {
    value == REDACTED || value == SECRET_REDACTED
}

fn json_path_mut<'a>(root: &'a mut Value, path: &[&str]) -> Option<&'a mut Value> {
    let mut cursor = root;
    for segment in path {
        cursor = cursor.as_object_mut()?.get_mut(*segment)?;
    }
    Some(cursor)
}

fn toml_path_mut<'a>(root: &'a mut toml::Value, path: &[&str]) -> Option<&'a mut toml::Value> {
    let mut cursor = root;
    for segment in path {
        cursor = cursor.as_table_mut()?.get_mut(*segment)?;
    }
    Some(cursor)
}

/// Test-only re-export of the recursive redaction pass so the cross-module
/// property test in `sensitive` can exercise this code path without going
/// through `redact()` (which requires a fully-populated `AletheiaConfig`).
#[cfg(test)]
pub(crate) fn redact_sensitive_keys_for_test(value: &mut Value) {
    redact_sensitive_keys(value);
}

fn redact_sensitive_keys(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, val) in map.iter_mut() {
                if key_is_sensitive(key) {
                    if val.is_string() {
                        *val = Value::String(REDACTED.to_owned());
                    }
                } else {
                    redact_sensitive_keys(val);
                }
            }
        }
        Value::Array(arr) => {
            for item in arr {
                redact_sensitive_keys(item);
            }
        }
        _ => {
            // NOTE: leaf values (null, bool, number, string) have no keys to redact
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    reason = "test: JSON string-key indexing; key presence is the assertion under test"
)]
mod tests {
    use super::*;

    #[test]
    fn redacts_gateway_signing_key() {
        let mut config = AletheiaConfig::default();
        config.gateway.auth.signing_key = Some(koina::secret::SecretString::from(
            "super-secret-jwt-signing-key",
        ));

        let redacted = redact(&config);
        assert_eq!(
            redacted["gateway"]["auth"]["signingKey"], REDACTED,
            "signing key should be redacted"
        );
        // INVARIANT: raw secret must not appear anywhere in the output
        assert!(
            !redacted
                .to_string()
                .contains("super-secret-jwt-signing-key"),
            "raw secret must not appear in redacted output"
        );
    }

    #[test]
    fn redacts_gateway_csrf_header_value() {
        let mut config = AletheiaConfig::default();
        config.gateway.csrf.header_value =
            koina::secret::SecretString::from("synthetic-csrf-header-secret");

        let redacted = redact(&config);
        assert_eq!(
            redacted["gateway"]["csrf"]["headerValue"], REDACTED,
            "csrf header value should be redacted"
        );
        assert!(
            !redacted
                .to_string()
                .contains("synthetic-csrf-header-secret"),
            "raw csrf header value must not appear in redacted output"
        );
    }

    #[test]
    fn redacts_tls_key_path() {
        let mut config = AletheiaConfig::default();
        config.gateway.tls.key_path = Some("/etc/ssl/private.key".to_owned());
        config.gateway.tls.cert_path = Some("/etc/ssl/cert.pem".to_owned());

        let redacted = redact(&config);
        assert_eq!(
            redacted["gateway"]["tls"]["keyPath"], REDACTED,
            "tls key path should be redacted"
        );
        assert_eq!(
            redacted["gateway"]["tls"]["certPath"], REDACTED,
            "tls cert path should be redacted"
        );
    }

    #[test]
    fn preserves_non_sensitive_fields() {
        let config = AletheiaConfig::default();
        let redacted = redact(&config);

        assert_eq!(
            redacted["gateway"]["port"], 18789,
            "non-sensitive gateway port should be preserved"
        );
        assert_eq!(
            redacted["agents"]["defaults"]["contextTokens"], 200_000,
            "non-sensitive context tokens should be preserved"
        );
        assert_eq!(
            redacted["embedding"]["provider"], "candle",
            "non-sensitive embedding provider should be preserved"
        );
    }

    #[test]
    fn result_is_valid_json_structure() {
        let config = AletheiaConfig::default();
        let redacted = redact(&config);
        assert!(
            redacted.is_object(),
            "redacted output should be a JSON object"
        );
        assert!(
            redacted["agents"].is_object(),
            "agents section should be a JSON object"
        );
        assert!(
            redacted["gateway"].is_object(),
            "gateway section should be a JSON object"
        );
    }
}
