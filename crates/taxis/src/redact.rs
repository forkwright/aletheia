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
//! Both retain the key and replace its value with a marker. The explicit passes
//! emit `"***"`; a typed [`SecretString`] whose field is not covered by either
//! pass retains its own `"[REDACTED]"` serialization marker. In both cases the
//! shape of a redacted config is identical to the real one.

use serde_json::Value;
use tracing::debug;

use koina::secret::SecretString;

use crate::config::{AletheiaConfig, ExternalToolAuth, ExternalToolEntry};
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

/// Failure while reconstructing a typed config from a redacted API payload.
///
/// The error deliberately does not include the rejected JSON path. Dynamic
/// config keys can themselves be operator-sensitive identifiers, and a
/// malformed marker must not turn an error response into a metadata leak.
#[derive(Debug)]
#[non_exhaustive]
pub enum RedactionMutationError {
    /// The typed config could not be converted to or from its mutation tree.
    Serialization(serde_json::Error),
    /// A reserved redaction marker appeared where the current config had not
    /// emitted one.
    UnexpectedMarker,
    /// The root config did not serialize as an object.
    InvalidRoot,
}

impl std::fmt::Display for RedactionMutationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Serialization(_) => f.write_str("failed to transform configuration"),
            Self::UnexpectedMarker => {
                f.write_str("redaction marker has no existing value to preserve")
            }
            Self::InvalidRoot => f.write_str("configuration root is not an object"),
        }
    }
}

impl std::error::Error for RedactionMutationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Serialization(source) => Some(source),
            Self::UnexpectedMarker | Self::InvalidRoot => None,
        }
    }
}

impl From<serde_json::Error> for RedactionMutationError {
    fn from(source: serde_json::Error) -> Self {
        Self::Serialization(source)
    }
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
    redacted_config_value(config).unwrap_or_else(|e| {
        debug!(error = %e, "failed to serialize config for redaction");
        Value::Null
    })
}

fn redacted_config_value(config: &AletheiaConfig) -> Result<Value, serde_json::Error> {
    let mut value = serde_json::to_value(config)?;
    redact_sensitive_leaves(&mut value);
    redact_sensitive_keys(&mut value);
    Ok(value)
}

/// Apply a redacted API patch to one top-level config section.
///
/// `"***"` is a placeholder only at paths where the redacted view of `staged`
/// emitted that marker. Such placeholders preserve the existing staged value,
/// including an absent optional (`null`). An explicit JSON `null` remains a
/// deliberate clear. A marker under a new dynamic key or any other sensitive
/// path is rejected instead of being persisted as data. Some [`SecretString`]
/// fields whose names are not themselves sensitive serialize as
/// `"[REDACTED]"`; that marker is accepted only when the current GET emitted it
/// at the exact same path.
///
/// This function also exposes [`SecretString`] values only inside its private
/// mutation tree. That keeps a GET/PUT round trip from replacing secrets with
/// `SecretString`'s serialization marker and preserves pending cold changes
/// from earlier PUTs.
///
/// # Errors
///
/// Returns [`RedactionMutationError`] when serialization fails, the config
/// root is malformed, or a reserved marker has no existing value to preserve.
pub fn apply_section_patch(
    staged: &AletheiaConfig,
    section: &str,
    patch: Value,
) -> Result<AletheiaConfig, RedactionMutationError> {
    apply_section_patch_with_marker_authority(staged, staged, section, patch)
}

/// Apply a redacted section patch while separating persistence and GET
/// authority.
///
/// `staged` is the disk-authoritative merge/replacement base, including cold
/// values deferred until restart. `marker_authority` is the live config from
/// which the client could actually have obtained a GET marker. A marker is
/// accepted only when that live GET emitted the exact marker at the same path;
/// its replacement still comes from `staged`, so an unrelated PUT preserves a
/// previously staged secret rotation.
///
/// # Errors
///
/// Returns [`RedactionMutationError`] when serialization fails, the config
/// root is malformed, or a marker was not emitted by `marker_authority`.
pub fn apply_section_patch_with_marker_authority(
    staged: &AletheiaConfig,
    marker_authority: &AletheiaConfig,
    section: &str,
    patch: Value,
) -> Result<AletheiaConfig, RedactionMutationError> {
    let mut root = config_value_with_exposed_secrets(staged)?;
    let Value::Object(root_map) = &mut root else {
        return Err(RedactionMutationError::InvalidRoot);
    };
    let existing = root_map.entry(section.to_owned()).or_insert(Value::Null);
    deep_merge(existing, patch);
    restore_redaction_markers_with_replacements(&mut root, staged, marker_authority)?;
    serde_json::from_value(root).map_err(Into::into)
}

/// Restore placeholders that came from the current config's redacted view.
///
/// A marker is accepted only when the same path in `current` would have been
/// redacted. This covers structural leaves, redact-only leaves, and dynamic
/// sensitive keys uniformly. Optional structural leaves that are currently
/// absent are restored to `null`; explicit `null` in `root` is never changed.
///
/// # Errors
///
/// Returns [`RedactionMutationError::UnexpectedMarker`] for a marker at a new
/// or otherwise non-preservable sensitive path.
pub fn restore_redaction_markers(
    root: &mut Value,
    current: &AletheiaConfig,
) -> Result<(), RedactionMutationError> {
    restore_redaction_markers_with_replacements(root, current, current)
}

fn restore_redaction_markers_with_replacements(
    root: &mut Value,
    replacements: &AletheiaConfig,
    marker_authority: &AletheiaConfig,
) -> Result<(), RedactionMutationError> {
    let replacement_raw = config_value_with_exposed_secrets(replacements)?;
    // This must be the actual GET representation, not a redaction pass over
    // `replacement_raw`. SecretString serialization itself emits `[REDACTED]` for
    // fields such as external header auth `value`, whose key name does not
    // trigger the generic sensitive-key pass.
    let current_redacted = redacted_config_value(marker_authority)?;

    restore_markers_at(
        root,
        Some(&replacement_raw),
        Some(&current_redacted),
        &mut Vec::new(),
    )
}

/// Serialize a config for internal comparison/mutation without erasing typed
/// secrets. The returned value must never be logged or returned from an API.
pub(crate) fn config_value_with_exposed_secrets(
    config: &AletheiaConfig,
) -> Result<Value, serde_json::Error> {
    let mut value = serde_json::to_value(config)?;
    visit_secret_strings(config, |path, secret| {
        if let Some(slot) = json_path_mut(&mut value, path) {
            *slot = Value::String(secret.expose_secret().to_owned());
        }
    });
    Ok(value)
}

fn deep_merge(base: &mut Value, patch: Value) {
    match (base, patch) {
        (Value::Object(base_map), Value::Object(patch_map)) => {
            for (key, patch_value) in patch_map {
                let entry = base_map.entry(key).or_insert(Value::Null);
                deep_merge(entry, patch_value);
            }
        }
        (base, patch) => *base = patch,
    }
}

fn restore_markers_at(
    staged: &mut Value,
    current_raw: Option<&Value>,
    current_redacted: Option<&Value>,
    path: &mut Vec<String>,
) -> Result<(), RedactionMutationError> {
    if is_redaction_marker(staged) {
        if current_redacted.is_some_and(|current| current == staged) {
            let Some(raw) = current_raw else {
                return Err(RedactionMutationError::UnexpectedMarker);
            };
            *staged = raw.clone();
            return Ok(());
        }
        if path_is_redactable(path) {
            return Err(RedactionMutationError::UnexpectedMarker);
        }
        return Ok(());
    }

    match staged {
        Value::Object(map) => {
            for (key, value) in map {
                path.push(key.clone());
                restore_markers_at(
                    value,
                    current_raw.and_then(|current| current.get(key)),
                    current_redacted.and_then(|current| current.get(key)),
                    path,
                )?;
                path.pop();
            }
        }
        Value::Array(values) => {
            for (index, value) in values.iter_mut().enumerate() {
                path.push(index.to_string());
                restore_markers_at(
                    value,
                    current_raw.and_then(|current| current.get(index)),
                    current_redacted.and_then(|current| current.get(index)),
                    path,
                )?;
                path.pop();
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
    Ok(())
}

fn path_is_redactable(path: &[String]) -> bool {
    SENSITIVE_LEAVES.iter().any(|leaf| {
        leaf.path.len() == path.len()
            && leaf
                .path
                .iter()
                .zip(path)
                .all(|(expected, actual)| expected == actual)
    }) || path.last().is_some_and(|key| key_is_sensitive(key))
        || matches!(
            path,
            [tools, class, _tool_id, auth, value]
                if tools == "tools"
                    && matches!(class.as_str(), "required" | "optional")
                    && auth == "auth"
                    && matches!(value.as_str(), "token" | "value")
        )
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
/// This compatibility helper is for trusted internal trees and preserves its
/// historical infallible API. Untrusted API input must use
/// [`apply_section_patch`], which rejects unresolved markers fail-closed.
pub fn preserve_secret_leaves(root: &mut Value, current: &AletheiaConfig) {
    if let Err(error) = restore_redaction_markers(root, current) {
        debug!(%error, "could not preserve invalid redaction marker");
    }
}

fn visit_secret_strings(config: &AletheiaConfig, mut visit: impl FnMut(&[&str], &SecretString)) {
    for leaf in SENSITIVE_LEAVES {
        match leaf.value {
            SensitiveLeafValue::Secret(accessor) => {
                if let Some(secret) = accessor(config) {
                    visit(leaf.path, secret);
                }
            }
            SensitiveLeafValue::RequiredSecret(accessor) => visit(leaf.path, accessor(config)),
            SensitiveLeafValue::RedactOnly => {}
        }
    }

    for (class, entries) in [
        ("required", &config.tools.required),
        ("optional", &config.tools.optional),
    ] {
        for (tool_id, entry) in entries {
            visit_external_tool_secret(class, tool_id, entry, &mut visit);
        }
    }
}

fn visit_external_tool_secret(
    class: &str,
    tool_id: &str,
    entry: &ExternalToolEntry,
    visit: &mut impl FnMut(&[&str], &SecretString),
) {
    match entry.auth.as_ref() {
        Some(ExternalToolAuth::Bearer { token }) => {
            visit(&["tools", class, tool_id, "auth", "token"], token);
        }
        Some(ExternalToolAuth::Header { value, .. }) => {
            visit(&["tools", class, tool_id, "auth", "value"], value);
        }
        Some(ExternalToolAuth::EnvToken { .. }) | None => {}
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
    let mut value = config_value_with_exposed_secrets(staged)?;
    preserve_secret_leaves(&mut value, current);
    serde_json::from_value(value)
}

pub(crate) fn expose_secret_leaves_for_toml(root: &mut toml::Value, current: &AletheiaConfig) {
    visit_secret_strings(current, |path, secret| {
        if let Some(slot) = toml_path_mut(root, path)
            && slot.as_str() == Some(SECRET_REDACTED)
        {
            *slot = toml::Value::String(secret.expose_secret().to_owned());
        }
    });
}

fn gateway_auth_signing_key(config: &AletheiaConfig) -> Option<&SecretString> {
    config.gateway.auth.signing_key.as_ref()
}

fn gateway_csrf_header_value(config: &AletheiaConfig) -> &SecretString {
    &config.gateway.csrf.header_value
}

fn is_redaction_marker(value: &Value) -> bool {
    matches!(value.as_str(), Some(REDACTED | SECRET_REDACTED))
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
    fn gateway_get_put_round_trip_preserves_absent_sensitive_options() {
        let current = AletheiaConfig::default();
        let patch = redact(&current)["gateway"].clone();

        let rebuilt = apply_section_patch(&current, "gateway", patch)
            .unwrap_or_else(|error| panic!("apply redacted gateway patch: {error}"));

        assert!(
            rebuilt.gateway.auth.signing_key.is_none(),
            "a marker emitted for an absent signing key must restore None"
        );
        assert!(
            rebuilt.gateway.tls.cert_path.is_none(),
            "a redact-only marker emitted for an absent cert path must restore None"
        );
        assert!(
            rebuilt.gateway.tls.key_path.is_none(),
            "a redact-only marker emitted for an absent key path must restore None"
        );
    }

    #[test]
    fn explicit_null_clears_optional_secret_instead_of_preserving_it() {
        let mut current = AletheiaConfig::default();
        current.gateway.auth.signing_key = Some(SecretString::from("old-signing-secret"));
        let mut patch = redact(&current)["gateway"].clone();
        patch["auth"]["signingKey"] = Value::Null;

        let rebuilt = apply_section_patch(&current, "gateway", patch)
            .unwrap_or_else(|error| panic!("apply explicit-null gateway patch: {error}"));

        assert!(
            rebuilt.gateway.auth.signing_key.is_none(),
            "JSON null is a deliberate clear, not a redaction placeholder"
        );
    }

    #[test]
    fn dynamic_matrix_marker_preserves_existing_environment_name() {
        let mut current = AletheiaConfig::default();
        current.channels.matrix.accounts.insert(
            "primary".to_owned(),
            crate::config::MatrixAccountConfig {
                homeserver: "https://matrix.example.org".to_owned(),
                access_token_env: "MATRIX_SYNTHETIC_TOKEN".to_owned(),
                ..crate::config::MatrixAccountConfig::default()
            },
        );
        let patch = redact(&current)["channels"].clone();

        let rebuilt = apply_section_patch(&current, "channels", patch)
            .unwrap_or_else(|error| panic!("apply redacted channels patch: {error}"));

        assert_eq!(
            rebuilt.channels.matrix.accounts["primary"].access_token_env, "MATRIX_SYNTHETIC_TOKEN",
            "dynamic sensitive-name redaction must not persist the marker"
        );
    }

    #[test]
    fn marker_under_new_dynamic_sensitive_path_is_rejected() {
        let current = AletheiaConfig::default();
        let patch = serde_json::json!({
            "matrix": {
                "accounts": {
                    "new-account": {
                        "homeserver": "https://matrix.example.org",
                        "accessTokenEnv": REDACTED
                    }
                }
            }
        });

        let error = apply_section_patch(&current, "channels", patch)
            .expect_err("a new account has no prior redacted value to preserve");
        assert!(matches!(error, RedactionMutationError::UnexpectedMarker));
    }

    #[test]
    fn staged_only_dynamic_marker_is_not_authorized_by_live_get() {
        let live = AletheiaConfig::default();
        let mut staged = live.clone();
        staged.channels.matrix.accounts.insert(
            "pending".to_owned(),
            crate::config::MatrixAccountConfig {
                homeserver: "https://matrix.example.org".to_owned(),
                access_token_env: "MATRIX_PENDING_TOKEN".to_owned(),
                ..crate::config::MatrixAccountConfig::default()
            },
        );
        let patch = serde_json::json!({
            "matrix": {
                "accounts": {
                    "pending": {
                        "accessTokenEnv": REDACTED
                    }
                }
            }
        });

        let error = apply_section_patch_with_marker_authority(&staged, &live, "channels", patch)
            .expect_err("disk-only account marker was never emitted by the live GET");
        assert!(matches!(error, RedactionMutationError::UnexpectedMarker));
    }

    #[test]
    fn dynamic_tool_auth_secret_survives_redacted_patch() {
        let mut current = AletheiaConfig::default();
        current.tools.optional.insert(
            "synthetic-search".to_owned(),
            ExternalToolEntry {
                auth: Some(ExternalToolAuth::Bearer {
                    token: SecretString::from("synthetic-tool-secret"),
                }),
                ..ExternalToolEntry::default()
            },
        );
        let patch = redact(&current)["tools"].clone();

        let rebuilt = apply_section_patch(&current, "tools", patch)
            .unwrap_or_else(|error| panic!("apply redacted tools patch: {error}"));
        let Some(ExternalToolAuth::Bearer { token }) = rebuilt
            .tools
            .optional
            .get("synthetic-search")
            .and_then(|entry| entry.auth.as_ref())
        else {
            panic!("rebuilt tool must retain bearer auth");
        };
        assert_eq!(token.expose_secret(), "synthetic-tool-secret");
    }

    #[test]
    fn dynamic_header_auth_secret_survives_its_exact_get_marker() {
        let mut current = AletheiaConfig::default();
        current.tools.optional.insert(
            "synthetic-search".to_owned(),
            ExternalToolEntry {
                auth: Some(ExternalToolAuth::Header {
                    name: "X-Synthetic-Key".to_owned(),
                    value: SecretString::from("synthetic-header-secret"),
                }),
                ..ExternalToolEntry::default()
            },
        );
        let patch = redact(&current)["tools"].clone();
        assert_eq!(
            patch["optional"]["synthetic-search"]["auth"]["value"], SECRET_REDACTED,
            "test must exercise SecretString's marker rather than key-name redaction"
        );

        let rebuilt = apply_section_patch(&current, "tools", patch)
            .unwrap_or_else(|error| panic!("apply redacted tools patch: {error}"));
        let Some(ExternalToolAuth::Header { value, .. }) = rebuilt
            .tools
            .optional
            .get("synthetic-search")
            .and_then(|entry| entry.auth.as_ref())
        else {
            panic!("rebuilt tool must retain header auth");
        };
        assert_eq!(value.expose_secret(), "synthetic-header-secret");
    }

    #[test]
    fn marker_under_new_dynamic_header_auth_path_is_rejected() {
        let current = AletheiaConfig::default();
        let patch = serde_json::json!({
            "optional": {
                "new-header-tool": {
                    "type": "http",
                    "endpoint": "https://tools.example.org/mcp",
                    "auth": {
                        "type": "header",
                        "name": "X-Synthetic-Key",
                        "value": SECRET_REDACTED
                    }
                }
            }
        });

        let error = apply_section_patch(&current, "tools", patch)
            .expect_err("a new header auth value has no GET marker to preserve");
        assert!(matches!(error, RedactionMutationError::UnexpectedMarker));
    }

    #[test]
    fn wrong_marker_for_existing_sensitive_path_is_rejected() {
        let mut current = AletheiaConfig::default();
        current.gateway.auth.signing_key = Some(SecretString::from("old-signing-secret"));
        let patch = serde_json::json!({
            "auth": { "signingKey": SECRET_REDACTED }
        });

        let error = apply_section_patch(&current, "gateway", patch)
            .expect_err("GET emits *** for signingKey, not SecretString's marker");
        assert!(matches!(error, RedactionMutationError::UnexpectedMarker));
    }

    #[test]
    fn marker_literal_at_non_sensitive_path_remains_data() {
        let current = AletheiaConfig::default();
        let rebuilt = apply_section_patch(
            &current,
            "embedding",
            serde_json::json!({ "model": REDACTED }),
        )
        .unwrap_or_else(|error| panic!("apply non-sensitive marker literal: {error}"));

        assert_eq!(rebuilt.embedding.model.as_deref(), Some(REDACTED));
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
