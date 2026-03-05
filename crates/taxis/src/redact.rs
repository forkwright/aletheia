//! Config redaction — strips secrets from config before API exposure.

use serde_json::Value;

use crate::config::AletheiaConfig;

const REDACTED: &str = "***";

/// Paths whose leaf values are replaced with `"***"` in redacted output.
const SENSITIVE_LEAVES: &[&[&str]] = &[
    &["gateway", "tls", "keyPath"],
    &["gateway", "tls", "certPath"],
];

/// Object keys at any depth whose values are unconditionally redacted.
const SENSITIVE_KEYS: &[&str] = &["token", "secret", "password", "apiKey", "api_key"];

/// Keys within Signal account objects that contain PII.
const SIGNAL_PII_KEYS: &[&str] = &["account"];

/// Serialize config to JSON, then redact sensitive fields.
pub fn redact(config: &AletheiaConfig) -> Value {
    let mut value = serde_json::to_value(config).unwrap_or(Value::Null);
    redact_sensitive_leaves(&mut value);
    redact_sensitive_keys(&mut value);
    redact_signal_accounts(&mut value);
    value
}

fn redact_sensitive_leaves(root: &mut Value) {
    for path in SENSITIVE_LEAVES {
        let json_pointer = format!("/{}", path.join("/"));
        if let Some(val) = root.pointer_mut(&json_pointer) {
            if val.is_string() || val.is_null() {
                *val = Value::String(REDACTED.to_owned());
            }
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
        _ => {}
    }
}

fn redact_signal_accounts(root: &mut Value) {
    let accounts = root
        .pointer_mut("/channels/signal/accounts")
        .and_then(Value::as_object_mut);

    if let Some(accounts_map) = accounts {
        for (_label, account) in accounts_map.iter_mut() {
            if let Value::Object(acct) = account {
                for key in SIGNAL_PII_KEYS {
                    if acct.contains_key(*key) {
                        acct.insert((*key).to_owned(), Value::String(REDACTED.to_owned()));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(redacted["agents"]["defaults"]["timeoutSeconds"], 300);
        assert_eq!(redacted["embedding"]["provider"], "mock");
    }

    #[test]
    fn redacts_signal_phone_numbers() {
        let mut config = AletheiaConfig::default();
        config.channels.signal.accounts.insert(
            "main".to_owned(),
            crate::config::SignalAccountConfig {
                account: Some("+15551234567".to_owned()),
                ..Default::default()
            },
        );

        let redacted = redact(&config);
        let account = &redacted["channels"]["signal"]["accounts"]["main"]["account"];
        assert_eq!(account, REDACTED);
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
