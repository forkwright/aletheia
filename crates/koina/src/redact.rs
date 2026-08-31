//! Sensitive value redaction for log output.
//!
//! Strips API keys (Anthropic `sk-ant-*`, generic `sk-*`), bearer tokens,
//! JWTs, and password-like key=value pairs from strings before they reach logs.
//!
//! Channel identities (phone numbers, Matrix IDs, account IDs) have two
//! redacted forms with distinct contracts:
//!
//! - [`redact_channel_id`] — lossy suffix form for human-facing display
//!   (logs, spans, debug output). Distinct identities alias; never use it
//!   as a correlation key.
//! - [`opaque_channel_id`] — collision-resistant hash form for correlation
//!   keys, map keys, and anything durable.

use std::sync::LazyLock;

use regex::Regex;
use sha2::{Digest as _, Sha256};

/// Compile a static regex from a literal pattern.
macro_rules! static_regex {
    ($name:ident, $pattern:expr) => {
        // WHY (#5603): these patterns are compile-time constants. A regex that
        // fails to compile is a programmer error; failing closed (panic on
        // first access) prevents silent credential leakage through logs.
        #[allow(
            clippy::expect_used,
            reason = "static regex patterns are compile-time constants and must be valid"
        )]
        static $name: LazyLock<Regex> =
            LazyLock::new(|| Regex::new($pattern).expect("BUG: static regex must compile"));
    };
}

static_regex!(RE_ANTHROPIC_KEY, r"sk-ant-api03-[A-Za-z0-9_-]+");
static_regex!(RE_SK_KEY, r"sk-[A-Za-z0-9_-]{20,}");
static_regex!(RE_BEARER, r"Bearer [A-Za-z0-9._-]+");
static_regex!(RE_JWT, r"eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+");
static_regex!(
    RE_SECRETS,
    // WHY (#6003): redact double-quoted and single-quoted values even when
    // they contain spaces, as well as unquoted non-whitespace values.
    r#"(?i)(password|secret|api_key|apikey)\s*[:=]\s*("[^"]*"|'[^']*'|\S+)"#
);

/// Redact sensitive values (API keys, JWTs, bearer tokens, passwords) from a string.
#[must_use]
pub fn redact_sensitive(value: &str) -> String {
    let mut result = replace_sensitive(&RE_BEARER, value, "Bearer ***");
    // JWT segments are base64url and can contain key-like substrings such as
    // `sk-...`; redact the full JWT before narrower API-key patterns split it.
    result = replace_sensitive(&RE_JWT, &result, "[JWT REDACTED]");
    result = replace_sensitive(&RE_ANTHROPIC_KEY, &result, "sk-ant-***");
    result = replace_sensitive(&RE_SK_KEY, &result, "sk-***");
    result = replace_sensitive(&RE_SECRETS, &result, "$1=***");
    result
}

fn replace_sensitive(regex: &LazyLock<Regex>, value: &str, replacement: &str) -> String {
    regex.replace_all(value, replacement).into_owned()
}

/// Canonical credential-bearing CLI flag names, checked after stripping
/// leading dashes and internal `-`/`_` separators (so `--api-key`,
/// `--api_key`, and `--apikey` all normalize to `apikey`).
///
/// WHY(#7020): union of the two argv-redaction policies this replaces
/// (`eval::provenance::is_secret_flag`, `agora::command::is_sensitive_arg_name`)
/// — `passphrase` was Agora-only and `ak-`/`pk-`-prefixed bare values were
/// Eval-only, so identical commands redacted differently depending on which
/// recorder observed them.
const SECRET_FLAG_NAMES: &[&str] = &[
    "apikey",
    "bearer",
    "passphrase",
    "password",
    "secret",
    "token",
    "key",
];

/// Subset of [`SECRET_FLAG_NAMES`] matched by suffix as well as by exact
/// name, so `--judge-api-key` and `--sig-token` are covered without a
/// generic `key`/`bearer`/`passphrase` suffix match turning `--monkey` or
/// `--turkey` into a false positive.
const SECRET_FLAG_SUFFIXES: &[&str] = &["apikey", "password", "secret", "token"];

/// Bare-value prefixes (case-insensitive) that mark an argv token as a
/// credential regardless of the preceding flag name.
///
/// WHY(#7020): union of `sk-`/`ak-`/`pk-`/`bearer ` (Eval) and `sk-`/`xox`/
/// `ghp_` (Agora).
const SECRET_VALUE_PREFIXES: &[&str] = &["sk-", "ak-", "pk-", "bearer ", "xox", "ghp_"];

/// Minimum length for an unprefixed token to be treated as an opaque secret
/// (long alphanumeric/`_`/`-`/`.` string, e.g. a bare API key or JWT-shaped
/// value with no recognizable flag or prefix).
const OPAQUE_SECRET_MIN_LEN: usize = 32;

/// True when `name` (a flag with leading dashes already stripped) names a
/// credential-bearing CLI option under the canonical policy.
#[must_use]
fn is_secret_flag_name(name: &str) -> bool {
    let normalized = name.to_ascii_lowercase().replace(['-', '_'], "");
    SECRET_FLAG_NAMES.contains(&normalized.as_str())
        || SECRET_FLAG_SUFFIXES
            .iter()
            .any(|suffix| normalized.ends_with(suffix))
}

/// True when `value` looks like a bare credential (no flag context): a
/// recognized key-prefix shape, or a long opaque alphanumeric/`_`/`-`/`.`
/// token.
#[must_use]
fn looks_like_secret_value(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    if SECRET_VALUE_PREFIXES
        .iter()
        .any(|prefix| lower.starts_with(prefix))
    {
        return true;
    }
    value.len() >= OPAQUE_SECRET_MIN_LEN
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

/// Redact secret-bearing CLI argument tokens under the canonical
/// flag/value-shape policy.
///
/// A token matching [`is_secret_flag_name`] (after stripping leading dashes,
/// and either as `--flag=value` or `--flag value`) has its value replaced
/// with `[REDACTED]`; the flag name itself is preserved. A bare token
/// matching [`looks_like_secret_value`] is replaced outright.
///
/// WHY(#7020): the single argv-redaction algorithm shared by Eval
/// (`EvalProvenance::redacted_args`) and Agora (`Command::redact_args`),
/// which previously reimplemented this with divergent policy tables —
/// identical commands persisted with different credential coverage
/// depending on which crate recorded them.
#[must_use]
pub fn redact_argv<'a>(tokens: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut out = Vec::new();
    let mut redact_next = false;

    for token in tokens {
        if redact_next {
            out.push("[REDACTED]".to_owned());
            redact_next = false;
            continue;
        }

        if let Some((flag, _value)) = token.split_once('=')
            && is_secret_flag_name(flag.trim_start_matches('-'))
        {
            out.push(format!("{flag}=[REDACTED]"));
            continue;
        }

        if is_secret_flag_name(token.trim_start_matches('-')) {
            out.push(token.to_owned());
            redact_next = true;
            continue;
        }

        if looks_like_secret_value(token) {
            out.push("[REDACTED]".to_owned());
            continue;
        }

        out.push(token.to_owned());
    }

    out
}

/// Number of trailing characters preserved by [`redact_channel_id`] as a
/// human-readability aid (enough to tell log lines apart at a glance in
/// the common case, not enough to re-identify a person).
const CHANNEL_ID_VISIBLE_SUFFIX: usize = 4;

/// Redact a channel identity value -- a phone number, Matrix ID
/// (`@user:server`), account ID, or any other provider-specific sender
/// identifier -- for human-facing display: logs, tracing spans, and
/// debug output.
///
/// **Display-only. This form is lossy and aliases: any two identities
/// sharing their trailing [`CHANNEL_ID_VISIBLE_SUFFIX`] characters
/// redact to the same string** (`@alice:example.org` and
/// `@bob:example.org` both become `....org`), **so it must never be
/// used as a correlation key, map key, or durable identifier.** Use
/// [`opaque_channel_id`] for those (#7101).
///
/// WHY(#5198): the single channel-identity redaction used everywhere an
/// external identifier reaches a log or serialized diagnostic, replacing
/// per-provider helpers (`agora::listener::redact_phone` covered only
/// Signal's numeric format) that left Matrix IDs and other channel
/// identities unredacted by omission.
///
/// Preserves the trailing [`CHANNEL_ID_VISIBLE_SUFFIX`] characters (by
/// Unicode scalar, not byte, so this never panics on a multi-byte
/// boundary); anything at or under that length is fully masked.
#[must_use]
pub fn redact_channel_id(id: &str) -> String {
    let chars: Vec<char> = id.chars().collect();
    if chars.len() > CHANNEL_ID_VISIBLE_SUFFIX {
        let tail: String = chars
            .get(chars.len() - CHANNEL_ID_VISIBLE_SUFFIX..)
            .map(|s| s.iter().collect())
            .unwrap_or_default();
        format!("...{tail}")
    } else {
        "****".to_owned()
    }
}

/// Domain-separation context prefix for [`opaque_channel_id`] digests.
///
/// Versioned so a future change to the handle encoding mints visibly
/// different handles instead of silently colliding with existing ones.
const OPAQUE_CHANNEL_ID_CONTEXT: &[u8] = b"aletheia.koina.channel-id.v1\0";

/// Collision-resistant opaque handle for a channel identity -- the form
/// to use for correlation keys, map keys, and anything durable.
///
/// Unlike [`redact_channel_id`], distinct identities cannot alias
/// except with cryptographically negligible probability: the identity
/// is digested with SHA-256 rather than truncated to a suffix. The
/// handle is stable (the same `(domain, id)` pair always yields the
/// same handle), irreversible, and leaks nothing of the raw value.
///
/// `domain` names the purpose minting the handle (e.g.
/// `"signal-account"`). Both fields are length-prefixed into the
/// digest, so handles minted for one purpose can never be confused
/// with handles minted for another and no `(domain, id)` pair can
/// collide with a different pair by shifting bytes across the field
/// boundary.
///
/// WHY(#7101): suffix redaction was used for `conversation_id` and
/// probe-detail map keys, silently merging unrelated identities that
/// share a four-character suffix. Correlation goes through this handle;
/// the suffix form remains for human-facing display only.
#[must_use]
pub fn opaque_channel_id(domain: &str, id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(OPAQUE_CHANNEL_ID_CONTEXT);
    for field in [domain, id] {
        hasher.update(field.len().to_string().as_bytes());
        hasher.update(b"\0");
        hasher.update(field.as_bytes());
        hasher.update(b"\0");
    }
    format!("h:{}", hex_lower(&hasher.finalize()))
}

/// Lowercase hex encoding without external dependencies or per-byte
/// allocation.
fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().saturating_mul(2));
    for &byte in bytes {
        out.push(hex_digit(byte >> 4));
        out.push(hex_digit(byte & 0x0f));
    }
    out
}

fn hex_digit(nibble: u8) -> char {
    char::from(match nibble {
        0..=9 => b'0' + nibble,
        10..=15 => b'a' + (nibble - 10),
        _ => b'?',
    })
}

/// JSON object key substrings (case-insensitive) treated as carrying a
/// channel identity, personal identifier, or attachment location when
/// found in a raw provider payload.
const PII_KEY_SUBSTRINGS: &[&str] = &[
    "number", "phone", "uuid", "sender", "source", "mxid", "user", "url", "name", "id", "address",
    "email",
];

fn key_is_pii(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    PII_KEY_SUBSTRINGS.iter().any(|s| lower.contains(s))
}

/// Recursively redact leaf string values anywhere beneath `value`,
/// preserving object/array structure. Used once a PII-shaped key has been
/// matched, so everything nested under it (e.g. a `source` object holding
/// both a `number` and a `uuid`) is covered even if a nested key itself
/// does not match [`PII_KEY_SUBSTRINGS`].
fn redact_json_leaf(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::String(s) => serde_json::Value::String(redact_channel_id(s)),
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), redact_json_leaf(v)))
                .collect(),
        ),
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(redact_json_leaf).collect())
        }
        other => other.clone(),
    }
}

/// Recursively redact a raw channel payload (a deserialized Signal
/// envelope or Matrix event) so it is safe to retain and serialize.
///
/// Walks every JSON object key; a key whose name matches
/// [`PII_KEY_SUBSTRINGS`] (phone numbers, Matrix/user IDs, sender/source
/// fields, names, attachment URLs) has every string beneath it replaced
/// via [`redact_channel_id`], everything else is preserved unredacted so
/// the retained value stays useful for structural diagnostics.
#[must_use]
pub fn redact_json_identifiers(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.iter()
                .map(|(k, v)| {
                    let redacted = if key_is_pii(k) {
                        redact_json_leaf(v)
                    } else {
                        redact_json_identifiers(v)
                    };
                    (k.clone(), redacted)
                })
                .collect(),
        ),
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(redact_json_identifiers).collect())
        }
        other => other.clone(),
    }
}

/// Redact `value` via [`redact_json_identifiers`] and return `None` when
/// its encoded size exceeds `max_bytes`.
///
/// WHY(#5198): raw provider-payload capture is opt-in and bounded --
/// callers use this so a payload that redaction cannot shrink enough
/// (e.g. very large attachment metadata) is dropped outright rather than
/// truncated mid-structure into invalid JSON.
#[must_use]
pub fn bounded_redacted_payload(
    value: &serde_json::Value,
    max_bytes: usize,
) -> Option<serde_json::Value> {
    let redacted = redact_json_identifiers(value);
    let encoded = serde_json::to_vec(&redacted).ok()?;
    if encoded.len() > max_bytes {
        None
    } else {
        Some(redacted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_anthropic_api_key() {
        let input = "using key sk-ant-api03-abcdef123456_789XYZ for requests"; // kanon:ignore SECURITY/hardcoded-openai-api-key + gitleaks:allow + trufflehog:ignore -- synthetic key shape used by redaction self-test; not a real credential
        let output = redact_sensitive(input);
        assert_eq!(output, "using key sk-ant-*** for requests");
    }

    #[test]
    fn redacts_generic_sk_key() {
        let input = "key: sk-proj-abcdefghij1234567890abcdef"; // kanon:ignore SECURITY/hardcoded-openai-api-key + gitleaks:allow + trufflehog:ignore -- synthetic key shape used by redaction self-test; not a real credential
        let output = redact_sensitive(input);
        assert_eq!(output, "key: sk-***");
    }

    #[test]
    fn redacts_bearer_token() {
        let input = "Authorization: Bearer abc123def456.ghi789";
        let output = redact_sensitive(input);
        assert_eq!(output, "Authorization: Bearer ***");
    }

    #[test]
    fn redacts_jwt() {
        let input = "token=eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";
        let output = redact_sensitive(input);
        assert!(output.contains("[JWT REDACTED]"));
        assert!(!output.contains("dozjgNryP4J3jVmNHl0w5N"));
    }

    #[test]
    fn redacts_jwt_with_key_like_segment() {
        let input = "token=eyJsk-AAAAAAAAAAAAAAAAAAAA.A.A";
        let output = redact_sensitive(input);
        assert_eq!(output, "token=[JWT REDACTED]");
    }

    #[test]
    fn redacts_password_patterns() {
        assert!(redact_sensitive("password=hunter2").contains("***"));
        assert!(redact_sensitive("secret: my-secret-value").contains("***"));
        assert!(redact_sensitive("api_key=sk123abc").contains("***"));
        assert!(redact_sensitive("APIKEY: tok_live_abc").contains("***"));
    }

    #[test]
    fn leaves_safe_strings_unchanged() {
        let safe = "normal log message with session_id=abc123";
        assert_eq!(redact_sensitive(safe), safe);
    }

    #[test]
    fn handles_empty_input() {
        assert_eq!(redact_sensitive(""), "");
    }

    #[test]
    #[should_panic(expected = "BUG: static regex must compile")]
    #[expect(
        clippy::invalid_regex,
        reason = "intentionally malformed regex to verify fail-closed behavior"
    )]
    fn invalid_regex_pattern_panics_fail_closed() {
        // WHY (#5603): a malformed static regex must never fall back to
        // returning the original string, which would leak credentials. This
        // test would *pass* (incorrectly) under the old fail-open code.
        static_regex!(RE_INVALID, r"(?<unclosed");
        let _ = replace_sensitive(&RE_INVALID, "secret", "***");
    }

    #[test]
    fn handles_multiple_sensitive_values() {
        let input = "key=sk-ant-api03-abc123 and password=secret123"; // kanon:ignore SECURITY/hardcoded-openai-api-key -- synthetic key shape used by redaction self-test; not a real credential
        let output = redact_sensitive(input);
        assert!(output.contains("sk-ant-***"));
        assert!(!output.contains("abc123"));
        assert!(!output.contains("secret123"));
    }

    #[test]
    fn redact_argv_token_next_value() {
        let args = ["--url", "http://localhost", "--token", "super-secret"];
        let redacted = redact_argv(args);
        assert!(redacted.contains(&"[REDACTED]".to_owned()));
        assert!(!redacted.contains(&"super-secret".to_owned()));
    }

    #[test]
    fn redact_argv_token_equals() {
        let redacted = redact_argv(["--token=super-secret"]);
        assert_eq!(redacted, vec!["--token=[REDACTED]"]);
    }

    #[test]
    fn redact_argv_judge_api_key_suffix() {
        let redacted = redact_argv(["--judge-api-key", "sk-abc123"]);
        assert_eq!(redacted, vec!["--judge-api-key", "[REDACTED]"]);
    }

    #[test]
    fn redact_argv_passphrase() {
        let redacted = redact_argv(["--passphrase", "hunter2"]);
        assert_eq!(redacted, vec!["--passphrase", "[REDACTED]"]);
    }

    #[test]
    fn redact_argv_keeps_safe_values() {
        let args = ["--url", "http://localhost", "--timeout", "30"];
        let expected: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
        assert_eq!(redact_argv(args), expected);
    }

    #[test]
    fn redact_argv_detects_ak_and_pk_prefixed_bare_values() {
        assert_eq!(redact_argv(["ak-abc123def456"]), vec!["[REDACTED]"]);
        assert_eq!(redact_argv(["pk-abc123def456"]), vec!["[REDACTED]"]);
    }

    #[test]
    fn redact_argv_detects_xox_and_ghp_prefixed_bare_values() {
        assert_eq!(redact_argv(["xoxb-abc123def456"]), vec!["[REDACTED]"]);
        assert_eq!(redact_argv(["ghp_abc123def456"]), vec!["[REDACTED]"]);
    }

    #[test]
    fn redact_argv_does_not_false_positive_on_key_suffixed_flag() {
        // WHY(#7020): "key" is exact-matched, not suffix-matched, so an
        // unrelated flag ending in "key" is left alone.
        let redacted = redact_argv(["--monkey", "banana"]);
        assert_eq!(redacted, vec!["--monkey", "banana"]);
    }

    #[test]
    fn redact_channel_id_keeps_trailing_digits_of_phone_number() {
        let redacted = redact_channel_id("+15550100");
        assert_eq!(redacted, "...0100");
        assert!(!redacted.contains("+1555"));
    }

    #[test]
    fn redact_channel_id_keeps_trailing_chars_of_matrix_id() {
        let redacted = redact_channel_id("@alice:example.org");
        assert_eq!(redacted, "....org");
        assert!(!redacted.contains("alice"));
    }

    #[test]
    fn redact_channel_id_fully_masks_short_values() {
        assert_eq!(redact_channel_id(""), "****");
        assert_eq!(redact_channel_id("ab"), "****");
        assert_eq!(redact_channel_id("abcd"), "****");
    }

    #[test]
    fn redact_channel_id_is_stable() {
        assert_eq!(
            redact_channel_id("+15550100"),
            redact_channel_id("+15550100")
        );
    }

    #[test]
    fn opaque_channel_id_distinguishes_identities_sharing_a_suffix() {
        // WHY(#7101): the suffix form aliases these pairs ("....org" and
        // "...0100" respectively); the opaque handle must not.
        assert_ne!(
            opaque_channel_id("matrix-account", "@alice:example.org"),
            opaque_channel_id("matrix-account", "@bob:example.org"),
        );
        assert_ne!(
            opaque_channel_id("signal-account", "+15550100"),
            opaque_channel_id("signal-account", "+19990100"),
        );
    }

    #[test]
    fn opaque_channel_id_is_stable() {
        assert_eq!(
            opaque_channel_id("signal-account", "+15550100"),
            opaque_channel_id("signal-account", "+15550100"),
        );
    }

    #[test]
    fn opaque_channel_id_separates_domains() {
        assert_ne!(
            opaque_channel_id("signal-account", "+15550100"),
            opaque_channel_id("send-target", "+15550100"),
        );
    }

    #[test]
    fn opaque_channel_id_length_prefix_blocks_field_boundary_shifts() {
        // WHY(#7101): without length prefixes, ("ab", "c") and ("a", "bc")
        // would feed identical bytes to the hasher.
        assert_ne!(opaque_channel_id("ab", "c"), opaque_channel_id("a", "bc"));
    }

    #[test]
    fn opaque_channel_id_is_a_prefixed_hex_digest_without_the_raw_value() {
        let handle = opaque_channel_id("signal-account", "+15550100");
        assert!(!handle.contains("+15550100"), "{handle}");
        let hex = handle.strip_prefix("h:").unwrap_or_default();
        assert_eq!(hex.len(), 64, "{handle}");
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()), "{handle}");
    }

    #[test]
    fn redact_channel_id_does_not_panic_on_multibyte_boundary() {
        // WHY(#5198): byte-index slicing (`&id[id.len() - 4..]`) can split a
        // multi-byte UTF-8 char and panic; char-based slicing must not.
        let redacted = redact_channel_id("@ドンとん:example.org");
        assert_eq!(redacted, "....org");
    }

    #[test]
    fn redact_json_identifiers_redacts_signal_style_source_fields() {
        let raw = serde_json::json!({
            "sourceNumber": "+15550100",
            "sourceUuid": "uuid-abc-123",
            "sourceName": "Alice",
            "dataMessage": {
                "message": "hello",
                "attachments": [{"id": "att-1", "url": "https://cdn.example/a.jpg"}],
            },
        });
        let redacted = redact_json_identifiers(&raw);
        let dump = redacted.to_string();
        assert!(!dump.contains("+15550100"), "{dump}");
        assert!(!dump.contains("uuid-abc-123"), "{dump}");
        assert!(!dump.contains("Alice"), "{dump}");
        assert!(!dump.contains("https://cdn.example/a.jpg"), "{dump}");
        // Non-PII structural field untouched.
        assert_eq!(
            redacted
                .pointer("/dataMessage/message")
                .and_then(serde_json::Value::as_str),
            Some("hello")
        );
    }

    #[test]
    fn redact_json_identifiers_redacts_matrix_style_sender() {
        let raw = serde_json::json!({
            "sender": "@alice:example.org",
            "content": {"body": "hi", "url": "mxc://example.org/abc"},
        });
        let redacted = redact_json_identifiers(&raw);
        let dump = redacted.to_string();
        assert!(!dump.contains("@alice:example.org"), "{dump}");
        assert!(!dump.contains("mxc://example.org/abc"), "{dump}");
        assert_eq!(
            redacted
                .pointer("/content/body")
                .and_then(serde_json::Value::as_str),
            Some("hi")
        );
    }

    #[test]
    fn redact_json_identifiers_preserves_non_pii_arrays() {
        let raw = serde_json::json!({"tags": ["a", "b", "c"]});
        assert_eq!(redact_json_identifiers(&raw), raw);
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn bounded_redacted_payload_returns_redacted_value_within_budget() {
        let raw = serde_json::json!({"sourceNumber": "+15550100", "text": "hi"});
        let bounded = bounded_redacted_payload(&raw, 4096).expect("within budget");
        assert!(!bounded.to_string().contains("+15550100"));
    }

    #[test]
    fn bounded_redacted_payload_drops_oversized_value() {
        let raw = serde_json::json!({"text": "x".repeat(100)});
        assert!(bounded_redacted_payload(&raw, 10).is_none());
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "proptest assertions")]
mod proptests {
    use proptest::collection::vec;
    use proptest::prelude::*;

    use super::*;

    const ALPHANUM_HYPHEN_UNDERSCORE: &str =
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-";
    const BASE64URL_CHARS: &str =
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-";
    const BEARER_CHARS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789._-";
    const SECRET_VALUE_CHARS: &str =
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-.~+/=";
    const QUOTED_VALUE_CHARS: &str =
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789 _-.~+/=";

    const WRAPPERS: &[&str] = &["", " ", "\"", "'", "\n"];
    const SECRET_KEYS: &[&str] = &["password", "secret", "api_key", "apikey"];
    const SEPARATORS: &[&str] = &["=", ":", " = ", " : "];

    fn wrapper() -> impl Strategy<Value = &'static str> {
        (0..WRAPPERS.len()).prop_map(|i| *WRAPPERS.get(i).unwrap())
    }

    fn secret_key() -> impl Strategy<Value = &'static str> {
        (0..SECRET_KEYS.len()).prop_map(|i| *SECRET_KEYS.get(i).unwrap())
    }

    fn separator() -> impl Strategy<Value = &'static str> {
        (0..SEPARATORS.len()).prop_map(|i| *SEPARATORS.get(i).unwrap())
    }

    fn secret_body(
        range: std::ops::Range<usize>,
        allowed: &'static str,
    ) -> impl Strategy<Value = String> {
        let char_count = allowed.chars().count();
        vec(
            (0..char_count).prop_map(move |i| {
                allowed
                    .chars()
                    .nth(i)
                    // WHY: `i` is drawn from `0..allowed.chars().count()`.
                    .unwrap()
            }),
            range,
        )
        .prop_map(|chars| chars.into_iter().collect())
    }

    proptest! {
        #[test]
        fn prop_redacts_anthropic_key(
            prefix in wrapper(),
            suffix in wrapper(),
            body in secret_body(1..128usize, ALPHANUM_HYPHEN_UNDERSCORE),
        ) {
            let secret = format!("sk-ant-api03-{body}");
            let input = format!("{prefix}{secret}{suffix}");
            let output = redact_sensitive(&input);
            prop_assert!(output.contains("sk-ant-***"), "placeholder missing: {}", output);
            // WHY (#6003): check the full key format is absent, not just body chars — single
            // chars from ALPHANUM_HYPHEN_UNDERSCORE can appear in "sk-ant-***" itself.
            prop_assert!(!output.contains(secret.as_str()), "secret leaked: {}", output);
        }

        #[test]
        fn prop_redacts_generic_sk_key(
            prefix in wrapper(),
            suffix in wrapper(),
            body in secret_body(20..128usize, ALPHANUM_HYPHEN_UNDERSCORE),
        ) {
            let secret = format!("sk-{body}");
            let input = format!("{prefix}{secret}{suffix}");
            let output = redact_sensitive(&input);
            prop_assert!(output.contains("sk-***"), "placeholder missing: {}", output);
            prop_assert!(!output.contains(&body), "secret body leaked: {}", output);
        }

        #[test]
        fn prop_leaves_short_sk_key_unredacted(
            prefix in wrapper(),
            suffix in wrapper(),
            body in secret_body(0..19usize, ALPHANUM_HYPHEN_UNDERSCORE),
        ) {
            let input = format!("{prefix}sk-{body}{suffix}");
            let output = redact_sensitive(&input);
            prop_assert_eq!(output, input);
        }

        #[test]
        fn prop_redacts_bearer_token(
            prefix in wrapper(),
            suffix in wrapper(),
            token in secret_body(1..128usize, BEARER_CHARS),
        ) {
            let input = format!("{prefix}Bearer {token}{suffix}");
            let output = redact_sensitive(&input);
            prop_assert!(output.contains("Bearer ***"), "placeholder missing: {}", output);
            // WHY (#6003): check the full bearer string is absent — single BEARER_CHARS
            // like "B","e","a","r" appear in the "Bearer ***" placeholder itself.
            prop_assert!(!output.contains(&format!("Bearer {token}")), "token leaked: {}", output);
        }

        #[test]
        fn prop_redacts_jwt(
            prefix in wrapper(),
            suffix in wrapper(),
            header in secret_body(1..64usize, BASE64URL_CHARS),
            payload in secret_body(1..64usize, BASE64URL_CHARS),
            signature in secret_body(1..64usize, BASE64URL_CHARS),
        ) {
            let token = format!("eyJ{header}.{payload}.{signature}");
            let input = format!("{prefix}{token}{suffix}");
            let output = redact_sensitive(&input);
            prop_assert!(
                output.contains("[JWT REDACTED]"),
                "placeholder missing: {}",
                output
            );
            // WHY (#6003): check the full token is absent rather than individual parts —
            // "[JWT REDACTED]" contains chars (J,W,T,R,E,D,A,C) that are valid BASE64URL
            // chars, so a single-char header/payload/signature would cause a false failure.
            prop_assert!(!output.contains(&token), "JWT token leaked: {}", output);
        }

        #[test]
        fn prop_redacts_key_value_secret(
            prefix in wrapper(),
            suffix in wrapper(),
            key in secret_key(),
            sep in separator(),
            value in secret_body(1..64usize, SECRET_VALUE_CHARS),
        ) {
            let input = format!("{prefix}{key}{sep}{value}{suffix}");
            let output = redact_sensitive(&input);
            prop_assert!(output.contains("***"), "placeholder missing: {}", output);
            // WHY (#6003): check the full key+sep+value context is absent — value chars like
            // "p" appear in the retained key name ("password=***") causing false failures.
            let full_secret = format!("{key}{sep}{value}");
            prop_assert!(!output.contains(&full_secret), "value leaked in context: {}", output);
        }

        #[test]
        fn prop_redacts_quoted_key_value_secret_with_spaces(
            prefix in wrapper(),
            suffix in wrapper(),
            key in secret_key(),
            sep in separator(),
            value in secret_body(1..64usize, QUOTED_VALUE_CHARS),
        ) {
            let input = format!("{prefix}{key}{sep}\"{value}\"{suffix}");
            let output = redact_sensitive(&input);
            prop_assert!(output.contains("***"), "placeholder missing: {}", output);
            // WHY (#6003): check the full key+sep+"value" context is absent — same
            // single-char leak as prop_redacts_key_value_secret.
            let full_secret = format!("{key}{sep}\"{value}\"");
            prop_assert!(!output.contains(&full_secret), "quoted value leaked: {}", output);
        }
    }
}
