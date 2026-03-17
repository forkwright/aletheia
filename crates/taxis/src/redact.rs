//! Config redaction: strips secrets from config before API exposure.

use serde_json::Value;
use tracing::debug;

use crate::config::AletheiaConfig;

const REDACTED: &str = "***";

/// Paths whose leaf values are replaced with `"***"` in redacted output.
const SENSITIVE_LEAVES: &[&[&str]] = &[
    &["gateway", "auth", "signingKey"],
    &["gateway", "tls", "keyPath"],
    &["gateway", "tls", "certPath"],
];

/// Object keys at any depth whose values are unconditionally redacted.
const SENSITIVE_KEYS: &[&str] = &["token", "secret", "password", "apiKey", "api_key"];

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
    for path in SENSITIVE_LEAVES {
        let json_pointer = format!("/{}", path.join("/"));
        if let Some(val) = root.pointer_mut(&json_pointer)
            && (val.is_string() || val.is_null())
        {
            *val = Value::String(REDACTED.to_owned());
        }
    }
}

fn redact_sensitive_keys(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, val) in map.iter_mut() {
                let key_lower = key.to_lowercase();
                if SENSITIVE_KEYS.iter().any(|s| key_lower.contains(s)) {
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
        // NOTE: leaf values (null, bool, number, string) have no keys to redact
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_gateway_signing_key() {
        let mut config = AletheiaConfig::default();
        config.gateway.auth.signing_key = Some("super-secret-jwt-signing-key".to_owned());

        let redacted = redact(&config);
        assert_eq!(redacted["gateway"]["auth"]["signingKey"], REDACTED);
        // INVARIANT: raw secret must not appear anywhere in the output
        assert!(
            !redacted
                .to_string()
                .contains("super-secret-jwt-signing-key")
        );
    }

    #[test]
    fn redacts_tls_key_path() {
        let mut config = AletheiaConfig::default();
        config.gateway.tls.key_path = Some("/etc/ssl/private.key".to_owned());
        config.gateway.tls.cert_path = Some("/etc/ssl/cert.pem".to_owned());

        let redacted = redact(&config);
        assert_eq!(redacted["gateway"]["tls"]["keyPath"], REDACTED);
        assert_eq!(redacted["gateway"]["tls"]["certPath"], REDACTED);
    }

    #[test]
    fn preserves_non_sensitive_fields() {
        let config = AletheiaConfig::default();
        let redacted = redact(&config);

        assert_eq!(redacted["gateway"]["port"], 18789);
        assert_eq!(redacted["agents"]["defaults"]["contextTokens"], 200_000);
        assert_eq!(redacted["embedding"]["provider"], "candle");
    }

    #[test]
    fn result_is_valid_json_structure() {
        let config = AletheiaConfig::default();
        let redacted = redact(&config);
        assert!(redacted.is_object());
        assert!(redacted["agents"].is_object());
        assert!(redacted["gateway"].is_object());
    }
}
