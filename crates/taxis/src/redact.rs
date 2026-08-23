//! Config redaction: strips secrets from config before API exposure.
//!
//! # What is omitted versus redacted
//!
//! WHY(#4571) stated explicitly: an operator looking at a redacted config needs to know
//! whether a field they cannot find is absent from the config or hidden from them.
//!
//! **Nothing is omitted.** [`redact`] serialises the whole [`AletheiaConfig`] and then
//! replaces values in place, so every field the config carries appears in the output.
//! A key that is not in a redacted response is not in the config either.
//!
//! Values are replaced by two mechanisms, and the difference matters when adding a
//! field:
//!
//! * **By structural path** -- [`SENSITIVE_LEAVES`] names exact JSON pointers, for
//!   leaves that are sensitive by *position* rather than by name (`gateway.csrf.
//!   headerValue` is not called "secret" anything). Adding a field here also opts it
//!   into at-rest encryption unless it is marked `RedactOnly`.
//! * **By key name** -- [`crate::sensitive::key_is_sensitive`] matches substrings
//!   (`token`, `secret`, `password`, `apikey`, ...) at any depth. This is deliberately
//!   over-broad: it errs toward redacting, which is the safe direction. It applies only
//!   to string values, because the same substrings appear in numeric fields --
//!   `max_output_tokens` matches `token`, and a token *count* must survive intact.
//!
//! Both replace the value with a marker (`"***"`) rather than dropping the key, so the
//! shape of a redacted config is identical to the real one.

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

/// Structural leaf paths whose values must be encrypted at rest, not merely
/// redacted on display.
///
/// WHY(#5349): `encrypt.rs`'s at-rest encryption pass matches sensitive *key
/// names* (`sensitive::key_is_sensitive`); a leaf like `gateway.csrf.headerValue`
/// is sensitive by structural position rather than by name and would
/// otherwise be silently excluded from encryption while still being redacted
/// on display -- exactly the divergence `sensitive.rs`'s module doc warns
/// against. `RedactOnly` leaves (e.g. TLS key/cert *paths*, not key material)
/// are excluded: they name a file on disk, not a secret value to encrypt.
pub(crate) fn encryptable_leaf_paths() -> impl Iterator<Item = &'static [&'static str]> {
    SENSITIVE_LEAVES
        .iter()
        .filter(|leaf| !matches!(leaf.value, SensitiveLeafValue::RedactOnly))
        .map(|leaf| leaf.path)
}

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

/// Mutable lookup by dotted path segments into a TOML value tree.
///
/// Shared with `encrypt.rs`'s structural at-rest encryption pass (#5349) so
/// path-walking logic has one implementation.
pub(crate) fn toml_path_mut<'a>(
    root: &'a mut toml::Value,
    path: &[&str],
) -> Option<&'a mut toml::Value> {
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
                // WHY(#4571) the recursion is not in an `else`: a sensitive-named key
                // whose value is an object or array used to be neither redacted nor
                // descended into, so anything sensitive beneath it was emitted verbatim.
                //
                // WHY the `is_string` guard stays: `key_is_sensitive` matches
                // substrings, so `max_output_tokens` matches "token". Without the type
                // check a token COUNT would be rewritten to "***". The guard is right;
                // the missing recursion was the gap.
                if key_is_sensitive(key) && val.is_string() {
                    *val = Value::String(REDACTED.to_owned());
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
    fn encryptable_leaf_paths_excludes_redact_only_leaves() {
        // WHY(#5349): TLS key/cert paths name a file on disk, not secret
        // material -- encrypting the path string would be meaningless, so the
        // at-rest encryption pass must not see them.
        let paths: Vec<&[&str]> = encryptable_leaf_paths().collect();
        assert!(
            paths.contains(&["gateway", "auth", "signingKey"].as_slice()),
            "signingKey must be an encryptable structural leaf"
        );
        assert!(
            paths.contains(&["gateway", "csrf", "headerValue"].as_slice()),
            "headerValue must be an encryptable structural leaf"
        );
        assert!(
            !paths.contains(&["gateway", "tls", "keyPath"].as_slice()),
            "tls keyPath is RedactOnly and must not be encrypted"
        );
        assert!(
            !paths.contains(&["gateway", "tls", "certPath"].as_slice()),
            "tls certPath is RedactOnly and must not be encrypted"
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

    /// A sensitive-named key holding an object must not shield what is inside it.
    ///
    /// WHY(#4571) this is the case that was leaking: the recursion used to live in an
    /// `else`, so a key matching a sensitive fragment whose value was NOT a string was
    /// neither redacted (the type check failed) nor descended into (the else was
    /// skipped). Everything beneath it was emitted verbatim.
    ///
    /// `AletheiaConfig` has no such field today -- every sensitive-named field is a
    /// string or a number -- so this exercises the predicate directly rather than
    /// through a config. That is deliberate: the hole is one ordinary refactor away
    /// from being reachable (`hooks_turn_token_budget: u32` becoming a struct would do
    /// it), and a test that waits for the config to grow the shape is a test that
    /// arrives after the leak.
    #[test]
    fn a_sensitive_key_holding_an_object_still_has_its_contents_redacted() {
        let mut value = serde_json::json!({
            "secrets": {
                "provider": { "apiKey": "sk-live-must-not-survive" },
                "note": "not sensitive"
            }
        });

        redact_sensitive_keys_for_test(&mut value);

        assert_eq!(
            value["secrets"]["provider"]["apiKey"], REDACTED,
            "a secret nested under a sensitive-named object must still be redacted"
        );
        assert_eq!(
            value["secrets"]["note"], "not sensitive",
            "recursing must not redact non-sensitive siblings"
        );
    }

    /// The same, one level deeper and through an array.
    #[test]
    fn sensitive_values_inside_arrays_under_sensitive_keys_are_redacted() {
        let mut value = serde_json::json!({
            "tokens": [
                { "password": "hunter2" },
                { "label": "keep me" }
            ]
        });

        redact_sensitive_keys_for_test(&mut value);

        assert_eq!(value["tokens"][0]["password"], REDACTED);
        assert_eq!(value["tokens"][1]["label"], "keep me");
    }

    /// A numeric field whose NAME matches a sensitive fragment must keep its value.
    ///
    /// WHY this is pinned beside the fix: `key_is_sensitive` is substring-matched, so
    /// `max_output_tokens` matches "token". The `is_string` guard is the only thing
    /// stopping a token COUNT from being rewritten to "***", and the recursion fix
    /// moves that guard into a compound condition -- exactly the kind of edit that
    /// could drop it. Real config fields with this shape:
    /// `bootstrap_max_tokens`, `context_tokens`, `hooks_turn_token_budget`,
    /// `max_recall_tokens`, `chars_per_token`.
    #[test]
    fn a_numeric_field_named_like_a_secret_keeps_its_value() {
        let mut value = serde_json::json!({
            "max_output_tokens": 4096,
            "chars_per_token": 4,
            "apiKey": "sk-should-go"
        });

        redact_sensitive_keys_for_test(&mut value);

        assert_eq!(
            value["max_output_tokens"], 4096,
            "a token count is not a token; redacting it would corrupt the config view"
        );
        assert_eq!(value["chars_per_token"], 4);
        assert_eq!(value["apiKey"], REDACTED, "the actual secret still goes");
    }
}
