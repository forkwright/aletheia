//! Session-scoped secret vault and credential substitution.
//!
//! Provides an in-memory store for short-lived credentials (AWS SSO keys,
//! DB tokens, API keys) that are referenced via placeholders in tool arguments
//! and substituted at dispatch time — after the model emits the tool call,
//! before the tool is invoked. The resolved value never appears in the
//! conversation transcript or in any outbound Anthropic payload.

use std::collections::HashMap;
use std::sync::RwLock;

use koina::secret::SecretString;
use snafu::Snafu;

/// Errors from secret vault operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
pub enum SecretError {
    /// The requested secret is not present in the vault.
    #[snafu(display("secret `{name}` not in session store"))]
    MissingSecret {
        /// Name of the missing secret.
        name: String,
    },
}

/// Thread-safe in-memory store for named secrets.
///
/// Scoped to the process lifetime (or shorter if explicitly cleared). Values
/// are held as [`SecretString`] to prevent accidental `Debug`/`Display`
/// leakage.
#[derive(Debug, Default)]
pub struct SecretVault {
    inner: RwLock<HashMap<String, SecretString>>,
}

impl SecretVault {
    /// Create an empty vault.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
        }
    }

    /// Store a secret under `name`.
    ///
    /// Overwrites any existing entry with the same name.
    pub fn store(&self, name: impl Into<String>, value: impl Into<SecretString>) {
        let mut guard = self.inner.write().unwrap_or_else(|poisoned| {
            tracing::warn!("secret vault lock poisoned, recovering inner state");
            poisoned.into_inner()
        });
        guard.insert(name.into(), value.into());
    }

    /// Retrieve a copy of the secret named `name`.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<SecretString> {
        let guard = self.inner.read().unwrap_or_else(|poisoned| {
            tracing::warn!("secret vault lock poisoned, recovering inner state");
            poisoned.into_inner()
        });
        guard.get(name).cloned()
    }

    /// Remove the secret named `name`, returning it if it existed.
    #[must_use]
    pub fn remove(&self, name: &str) -> Option<SecretString> {
        let mut guard = self.inner.write().unwrap_or_else(|poisoned| {
            tracing::warn!("secret vault lock poisoned, recovering inner state");
            poisoned.into_inner()
        });
        guard.remove(name)
    }

    /// List all stored secret names (values are never exposed).
    #[must_use]
    pub fn list_names(&self) -> Vec<String> {
        let guard = self.inner.read().unwrap_or_else(|poisoned| {
            tracing::warn!("secret vault lock poisoned, recovering inner state");
            poisoned.into_inner()
        });
        guard.keys().cloned().collect()
    }

    /// Clear every entry from the vault.
    pub fn clear(&self) {
        let mut guard = self.inner.write().unwrap_or_else(|poisoned| {
            tracing::warn!("secret vault lock poisoned, recovering inner state");
            poisoned.into_inner()
        });
        guard.clear();
    }

    /// Number of secrets currently stored.
    #[must_use]
    pub fn len(&self) -> usize {
        let guard = self.inner.read().unwrap_or_else(|poisoned| {
            tracing::warn!("secret vault lock poisoned, recovering inner state");
            poisoned.into_inner()
        });
        guard.len()
    }

    /// Whether the vault contains no secrets.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Substitute `{{secret:<name>}}` and `$SECRET(<name>)` placeholders in a
/// JSON value with the corresponding secret from `vault`.
///
/// Substitution is recursive: it descends into objects and arrays.
/// If a placeholder references a missing secret, returns [`SecretError::MissingSecret`].
///
/// # Security note
///
/// This mutates `value` in place. Callers should clone the original if the
/// placeholder-bearing JSON is needed for persistence (e.g. transcript storage).
pub fn substitute_in_json(
    value: &mut serde_json::Value,
    vault: &SecretVault,
) -> Result<(), SecretError> {
    match value {
        serde_json::Value::String(s) => {
            if let Some(name) = parse_placeholder(s) {
                let secret = vault.get(name).ok_or_else(|| SecretError::MissingSecret {
                    name: name.to_owned(),
                })?;
                secret.expose_secret().clone_into(s);
            }
        }
        serde_json::Value::Object(map) => {
            for v in map.values_mut() {
                substitute_in_json(v, vault)?;
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr.iter_mut() {
                substitute_in_json(v, vault)?;
            }
        }
        // Numbers, bools, and null values contain no placeholders to substitute.
        _ => {}
    }
    Ok(())
}

/// Parse a placeholder string and return the secret name if it matches.
///
/// Supported forms:
/// - `{{secret:aws-sso}}` → `Some("aws-sso")`
/// - `$SECRET(aws-sso)` → `Some("aws-sso")`
fn parse_placeholder(s: &str) -> Option<&str> {
    // {{secret:name}}
    if let Some(inner) = s.strip_prefix("{{secret:")
        && let Some(name) = inner.strip_suffix("}}")
    {
        return Some(name);
    }

    // $SECRET(name)
    if let Some(inner) = s.strip_prefix("$SECRET(")
        && let Some(name) = inner.strip_suffix(")")
    {
        return Some(name);
    }

    None
}

/// Redact likely-secret string values inside a JSON value, replacing them
/// with `"[REDACTED]"`.
///
/// This is defense-in-depth: if a secret value leaks into a JSON payload
/// (e.g. via a tool result), the redaction pass prevents it from flowing
/// outward to logs or LLM providers.
///
/// The heuristic is conservative: strings longer than 32 characters that
/// contain no whitespace and are not already placeholders are treated as
/// sensitive.
pub fn redact_in_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(s) if looks_like_secret(s) => {
            "[REDACTED]".clone_into(s);
        }
        serde_json::Value::Object(map) => {
            for v in map.values_mut() {
                redact_in_json(v);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr.iter_mut() {
                redact_in_json(v);
            }
        }
        // Non-string scalars (numbers, bools, null) have no secret content to redact.
        _ => {}
    }
}

/// Heuristic: treat long alphanumeric strings without whitespace as sensitive.
fn looks_like_secret(s: &str) -> bool {
    if s.len() <= 32 {
        return false;
    }
    if parse_placeholder(s).is_some() {
        return false;
    }
    // If it contains whitespace it's probably prose, not a credential.
    if s.chars().any(char::is_whitespace) {
        return false;
    }
    true
}

#[cfg(test)]
#[expect(clippy::indexing_slicing, reason = "test assertions")]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn vault_round_trip() {
        let vault = SecretVault::new();
        vault.store("aws", "AKIAIOSFODNN7EXAMPLE");
        assert_eq!(
            vault.get("aws").unwrap().expose_secret(),
            "AKIAIOSFODNN7EXAMPLE"
        );
    }

    #[test]
    fn vault_list_shows_names_only() {
        let vault = SecretVault::new();
        vault.store("a", "secret-a");
        vault.store("b", "secret-b");
        let mut names = vault.list_names();
        names.sort();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn vault_remove() {
        let vault = SecretVault::new();
        vault.store("x", "val");
        assert!(vault.remove("x").is_some());
        assert!(vault.get("x").is_none());
    }

    #[test]
    fn vault_clear() {
        let vault = SecretVault::new();
        vault.store("x", "val");
        vault.clear();
        assert!(vault.is_empty());
    }

    #[test]
    fn substitute_brace_placeholder() {
        let vault = SecretVault::new();
        vault.store("token", "real-token-123");
        let mut value = serde_json::json!({"auth": "{{secret:token}}"});
        substitute_in_json(&mut value, &vault).unwrap();
        assert_eq!(value, serde_json::json!({"auth": "real-token-123"}));
    }

    #[test]
    fn substitute_dollar_placeholder() {
        let vault = SecretVault::new();
        vault.store("token", "real-token-123");
        let mut value = serde_json::json!({"auth": "$SECRET(token)"});
        substitute_in_json(&mut value, &vault).unwrap();
        assert_eq!(value, serde_json::json!({"auth": "real-token-123"}));
    }

    #[test]
    fn substitute_nested() {
        let vault = SecretVault::new();
        vault.store("a", "A");
        vault.store("b", "B");
        let mut value = serde_json::json!({"items": [{"k": "{{secret:a}}"}, {"k": "$SECRET(b)"}]});
        substitute_in_json(&mut value, &vault).unwrap();
        assert_eq!(
            value,
            serde_json::json!({"items": [{"k": "A"}, {"k": "B"}]})
        );
    }

    #[test]
    fn substitute_missing_secret_error() {
        let vault = SecretVault::new();
        let mut value = serde_json::json!({"auth": "{{secret:missing}}"});
        let err = substitute_in_json(&mut value, &vault).unwrap_err();
        match err {
            SecretError::MissingSecret { name } => assert_eq!(name, "missing"),
        }
    }

    #[test]
    fn substitute_leaves_plain_strings() {
        let vault = SecretVault::new();
        let mut value = serde_json::json!({"msg": "hello world"});
        substitute_in_json(&mut value, &vault).unwrap();
        assert_eq!(value, serde_json::json!({"msg": "hello world"}));
    }

    #[test]
    fn redact_long_secret_like_strings() {
        let mut value = serde_json::json!({
            "public": "hello",
            "secret": "thisisaverylongsecretvaluethatshouldberedacted123"
        });
        redact_in_json(&mut value);
        assert_eq!(value["public"], "hello");
        assert_eq!(value["secret"], "[REDACTED]");
    }

    #[test]
    fn redact_preserves_placeholders() {
        let mut value = serde_json::json!({"auth": "{{secret:aws}}"});
        redact_in_json(&mut value);
        assert_eq!(value["auth"], "{{secret:aws}}");
    }

    #[test]
    fn redact_skips_short_strings() {
        let mut value = serde_json::json!({"token": "short"});
        redact_in_json(&mut value);
        assert_eq!(value["token"], "short");
    }

    #[test]
    fn redact_skips_strings_with_whitespace() {
        let mut value = serde_json::json!({"text": "this is a long sentence with spaces in it ok"});
        redact_in_json(&mut value);
        assert_eq!(
            value["text"],
            "this is a long sentence with spaces in it ok"
        );
    }
}
