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
// kanon:ignore RUST/no-debug-derive-on-public-types WHY: Snafu macro requires Debug derive; error type contains no secret values
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

/// Redact values in `resolved` whose corresponding position in `template`
/// was a secret-vault placeholder.
///
/// This is used when a caller must show a resolved payload on a live-only
/// surface without disclosing even short vault values that the generic content
/// heuristic cannot recognize. For a separately tool-prepared value, use
/// [`redact_resolved_secrets_in_prepared_json`] so taint survives key/shape
/// normalization.
pub fn redact_resolved_secrets_in_json(
    template: &serde_json::Value,
    resolved: &mut serde_json::Value,
) {
    let substituted = resolved.clone();
    redact_resolved_secrets_in_prepared_json(template, &substituted, resolved);
}

/// Redact vault values from a tool-prepared JSON value while preserving their
/// provenance across tool-owned normalization.
///
/// `template` is the original placeholder-form input, `substituted` is its
/// shape-identical post-vault copy, and `prepared` is the executor-bound value
/// after registry-owned, shape-preserving path canonicalization. Placeholder
/// positions are redacted regardless of how their string changed; recovered
/// values are also scrubbed recursively as defense in depth.
pub fn redact_resolved_secrets_in_prepared_json(
    template: &serde_json::Value,
    substituted: &serde_json::Value,
    prepared: &mut serde_json::Value,
) {
    let mut secrets = Vec::new();
    collect_resolved_secret_values(template, substituted, &mut secrets);
    redact_placeholder_positions(template, prepared);
    redact_secret_values(prepared, &secrets);
}

fn redact_placeholder_positions(template: &serde_json::Value, prepared: &mut serde_json::Value) {
    match (template, prepared) {
        (serde_json::Value::String(template), prepared)
            if parse_placeholder(template).is_some() =>
        {
            *prepared = serde_json::Value::String("[REDACTED]".to_owned());
        }
        (serde_json::Value::Object(template), serde_json::Value::Object(prepared)) => {
            for (key, template_value) in template {
                if let Some(prepared_value) = prepared.get_mut(key) {
                    redact_placeholder_positions(template_value, prepared_value);
                }
            }
        }
        (serde_json::Value::Array(template), serde_json::Value::Array(prepared)) => {
            for (template_value, prepared_value) in template.iter().zip(prepared.iter_mut()) {
                redact_placeholder_positions(template_value, prepared_value);
            }
        }
        _ => {}
    }
}

fn collect_resolved_secret_values(
    template: &serde_json::Value,
    substituted: &serde_json::Value,
    secrets: &mut Vec<String>,
) {
    match (template, substituted) {
        (serde_json::Value::String(template), serde_json::Value::String(substituted))
            if parse_placeholder(template).is_some() =>
        {
            if !secrets.iter().any(|secret| secret == substituted) {
                secrets.push(substituted.clone());
            }
        }
        (serde_json::Value::Object(template), serde_json::Value::Object(substituted)) => {
            for (key, template_value) in template {
                if let Some(substituted_value) = substituted.get(key) {
                    collect_resolved_secret_values(template_value, substituted_value, secrets);
                }
            }
        }
        (serde_json::Value::Array(template), serde_json::Value::Array(substituted)) => {
            for (template_value, substituted_value) in template.iter().zip(substituted.iter()) {
                collect_resolved_secret_values(template_value, substituted_value, secrets);
            }
        }
        _ => {}
    }
}

fn redact_secret_values(value: &mut serde_json::Value, secrets: &[String]) {
    match value {
        serde_json::Value::String(text) => {
            for secret in secrets {
                if secret.is_empty() {
                    if text.is_empty() {
                        "[REDACTED]".clone_into(text);
                    }
                } else if text.contains(secret.as_str()) {
                    *text = text.replace(secret.as_str(), "[REDACTED]");
                }
            }
        }
        serde_json::Value::Object(map) => {
            if map.keys().any(|key| {
                secrets
                    .iter()
                    .any(|secret| !secret.is_empty() && key.contains(secret.as_str()))
            }) {
                *value = serde_json::json!({"__redaction__": "[REDACTED]"});
                return;
            }
            for child in map.values_mut() {
                redact_secret_values(child, secrets);
            }
        }
        serde_json::Value::Array(values) => {
            for child in values {
                redact_secret_values(child, secrets);
            }
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
    }
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
        serde_json::Value::String(s) => {
            // Catch definite credential syntax even inside ordinary prose.
            // The length-only fallback below intentionally remains more
            // conservative for durable copies.
            *s = koina::redact::redact_sensitive(s);
            if looks_like_secret(s) {
                "[REDACTED]".clone_into(s);
            }
        }
        serde_json::Value::Object(map) => {
            // SECURITY(#5015): object keys are data too. A model-controlled
            // map can put a credential in a dynamic key, where walking only
            // values would miss it and later schema/debug dumps would retain
            // it. Collapse the object if any key is secret-shaped: rewriting
            // keys individually can collide and silently discard entries.
            if map
                .keys()
                .any(|key| looks_like_secret(key) || koina::redact::redact_sensitive(key) != *key)
            {
                *value = serde_json::json!({"__redaction__": "[REDACTED]"});
                return;
            }
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

/// Redact known secret patterns, then bound the result to `max_bytes`.
///
/// WHY(#5261): the single required path for provider diagnostics — raw
/// provider bodies, SSE lines, and subprocess stderr all carry prompt
/// fragments, tool payloads, or credential-shaped strings that must not
/// reach a log line or a [`crate::error::Error`] returned to a caller
/// unsanitized. Redaction runs before truncation: truncating first can
/// split a token across the boundary and leave a partial secret
/// unredacted on either side of the cut.
pub(crate) fn sanitize_provider_text(text: &str, max_bytes: usize) -> String {
    let redacted = koina::redact::redact_sensitive(text);
    truncate_error_body(&redacted, max_bytes)
}

/// Truncate a provider error body to at most `max_bytes` bytes for safe logging.
///
/// Appends `[truncated N bytes]` when the body is truncated so operators know the log
/// entry is incomplete. Walks back to a UTF-8 character boundary so the
/// truncated slice is always valid text.
pub(crate) fn truncate_error_body(body: &str, max_bytes: usize) -> String {
    if body.len() <= max_bytes {
        return body.to_owned();
    }
    // Walk back from max_bytes to the nearest char boundary.
    let mut end = max_bytes;
    while end > 0 && !body.is_char_boundary(end) {
        end -= 1;
    }
    let remaining = body.len() - end;
    let prefix = body.get(..end).unwrap_or_default();
    format!("{prefix}[truncated {remaining} bytes]")
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
        vault.store("aws", "AKIAIOSFODNN7EXAMPLE"); // pii-allow: AWS canonical example key from docs.aws.amazon.com // kanon:ignore SECURITY/hardcoded-aws-access-key -- synthetic test fixture only
        assert_eq!(
            vault.get("aws").unwrap().expose_secret(),
            "AKIAIOSFODNN7EXAMPLE" // pii-allow: AWS canonical example key from docs.aws.amazon.com // kanon:ignore SECURITY/hardcoded-aws-access-key -- synthetic test fixture only
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

    #[test]
    fn redact_strong_credential_patterns_inside_prose() {
        let api_key = format!("{}{}", "sk-ant-api03-", "synthetic-redaction-key");
        let bearer = "Bearer synthetic.redaction.token";
        let jwt = format!(
            "{}.{}.{}",
            "eyJhbGciOiJIUzI1NiJ9", "c3ludGhldGlj", "c2lnbmF0dXJl"
        );
        let mut value = serde_json::json!({
            "text": format!("key={api_key}; auth={bearer}; jwt={jwt}"),
        });

        redact_in_json(&mut value);

        let redacted = value["text"].as_str().expect("redacted text");
        assert!(!redacted.contains(&api_key));
        assert!(!redacted.contains(bearer));
        assert!(!redacted.contains(&jwt));
        assert!(redacted.contains("sk-ant-***"));
        assert!(redacted.contains("Bearer ***"));
        assert!(redacted.contains("[JWT REDACTED]"));
    }

    #[test]
    fn redact_applies_long_token_fallback_after_strong_pattern_sanitization() {
        let opaque = "x".repeat(40);
        let recognized = format!("{}{}", "sk-ant-api03-", "synthetic-key");
        let mut value = serde_json::json!({"token": format!("{recognized}:{opaque}")});

        redact_in_json(&mut value);

        assert_eq!(value["token"], "[REDACTED]");
        assert!(!value.to_string().contains(&opaque));
    }

    #[test]
    fn redact_collapses_secret_shaped_dynamic_object_keys() {
        let secret_key = "dynamic-token-abcdefghijklmnopqrstuvwxyz0123456789";
        let mut value = serde_json::json!({(secret_key): "ordinary value", "safe": "visible"});

        redact_in_json(&mut value);

        assert_eq!(value, serde_json::json!({"__redaction__": "[REDACTED]"}));
        assert!(!value.to_string().contains(secret_key));
    }

    #[test]
    fn redact_collapses_short_and_whitespace_bearing_credential_keys() {
        for secret_key in [
            "password=hunter2",
            "Authorization: Bearer synthetic.dynamic.token",
        ] {
            let mut value = serde_json::json!({(secret_key): "ordinary value"});

            redact_in_json(&mut value);

            assert_eq!(value, serde_json::json!({"__redaction__": "[REDACTED]"}));
            assert!(!value.to_string().contains(secret_key));
        }
    }

    #[test]
    fn redact_resolved_secret_paths_hides_short_values_without_touching_file_refs() {
        let template = serde_json::json!({
            "auth": "{{secret:token}}",
            "nested": ["$SECRET(pin)", "{{file:payload.txt}}"],
        });
        let mut resolved = serde_json::json!({
            "auth": "short",
            "nested": ["1234", "expanded approval prose"],
        });

        redact_resolved_secrets_in_json(&template, &mut resolved);

        assert_eq!(resolved["auth"], "[REDACTED]");
        assert_eq!(resolved["nested"][0], "[REDACTED]");
        assert_eq!(resolved["nested"][1], "expanded approval prose");
    }

    #[test]
    fn redact_resolved_secret_taint_survives_tool_owned_restructuring() {
        let template = serde_json::json!({"password": "{{secret:token}}"});
        let substituted = serde_json::json!({"password": "1234"});
        let mut prepared = serde_json::json!({
            "authorization": "Bearer 1234",
            "nested": {"1234-dynamic": "value"},
        });

        redact_resolved_secrets_in_prepared_json(&template, &substituted, &mut prepared);

        assert_eq!(prepared["authorization"], "Bearer [REDACTED]");
        assert_eq!(
            prepared["nested"],
            serde_json::json!({"__redaction__": "[REDACTED]"})
        );
        assert!(!prepared.to_string().contains("1234"));
    }

    #[test]
    fn sanitize_provider_text_redacts_secret_before_truncating() {
        // WHY(#5261): a token straddling the truncation boundary must be
        // fully redacted, not half-truncated into a leaking fragment.
        let body = format!("prefix sk-{} suffix", "a".repeat(40));
        let sanitized = sanitize_provider_text(&body, 20);
        assert!(
            !sanitized.contains("aaaaaaaaaaaaaaaaaaaa"),
            "secret body must not survive sanitization: {sanitized}"
        );
    }

    #[test]
    fn sanitize_provider_text_truncates_long_safe_text() {
        let body = "x".repeat(600);
        let sanitized = sanitize_provider_text(&body, 500);
        assert!(sanitized.contains("[truncated "));
    }

    #[test]
    fn sanitize_provider_text_leaves_short_safe_text_unchanged() {
        let body = "HTTP 429: rate limited, retry later";
        assert_eq!(sanitize_provider_text(body, 512), body);
    }

    #[test]
    fn truncate_error_body_short_body_unchanged() {
        let body = r#"{"error":"not found"}"#;
        assert_eq!(truncate_error_body(body, 500), body);
    }

    #[test]
    fn truncate_error_body_long_body_truncated_with_marker() {
        let body = "x".repeat(600);
        let result = truncate_error_body(&body, 500);
        assert!(result.starts_with(&"x".repeat(500)));
        assert!(
            result.contains("[truncated "),
            "must include truncation marker"
        );
        assert!(result.contains("bytes]"), "must include byte count");
    }

    #[test]
    fn truncate_error_body_respects_utf8_boundary() {
        // 3-byte UTF-8 char at position 499 would split a char if not handled.
        let mut body = "a".repeat(498);
        body.push('€'); // 3 bytes (0xe2, 0x82, 0xac)
        body.push_str(&"z".repeat(100));
        let result = truncate_error_body(&body, 500);
        // The euro sign starts at byte 498 and ends at 501 — max_bytes=500
        // falls inside it. We must back up to 498, not split the char.
        assert!(std::str::from_utf8(result.as_bytes().get(..498).unwrap_or(b"")).is_ok());
        assert!(
            result.contains("[truncated "),
            "must include truncation marker"
        );
    }
}
