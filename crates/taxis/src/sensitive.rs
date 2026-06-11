//! Single source of truth for the "is this key name sensitive?" predicate.
//!
//! WHY(#4254): the redact-on-display path (`redact.rs`) and the
//! encrypt-at-rest path (`encrypt.rs`) must share one list; divergent lists
//! let a key be redacted on display yet stored in plaintext at rest.
//!
//! Failure-mode bias: substring matching errs on the side of MORE encryption /
//! MORE redaction, which is the safe direction for both code paths.

/// Lower-cased substrings that, if present in a config key name, mark the
/// value as sensitive — for both redaction and at-rest encryption.
pub(crate) const SENSITIVE_KEY_FRAGMENTS: &[&str] = &[
    "token",
    "secret",
    "password",
    "apikey",
    "api_key",
    "signingkey",
    "signing_key",
];

/// Returns true if `key` should be redacted on display AND encrypted at rest.
#[must_use]
pub(crate) fn key_is_sensitive(key: &str) -> bool {
    let lower = key.to_lowercase();
    SENSITIVE_KEY_FRAGMENTS.iter().any(|s| lower.contains(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_canonical_names() {
        assert!(key_is_sensitive("api_key"));
        assert!(key_is_sensitive("apiKey"));
        assert!(key_is_sensitive("password"));
        assert!(key_is_sensitive("secret"));
        assert!(key_is_sensitive("token"));
        assert!(key_is_sensitive("signingKey"));
        assert!(key_is_sensitive("signing_key"));
    }

    #[test]
    fn matches_prefixed_and_suffixed_variants() {
        // WHY(#4254): prefixed/suffixed variants were redacted on display but
        // NOT encrypted at rest; substring matching must catch them.
        assert!(key_is_sensitive("auth_token"));
        assert!(key_is_sensitive("authToken"));
        assert!(key_is_sensitive("refresh_token"));
        assert!(key_is_sensitive("refreshToken"));
        assert!(key_is_sensitive("session_secret"));
        assert!(key_is_sensitive("sessionSecret"));
        assert!(key_is_sensitive("openai_api_key"));
        assert!(key_is_sensitive("anthropicApiKey"));
        assert!(key_is_sensitive("admin_password"));
        assert!(key_is_sensitive("jwtSigningKey"));
    }

    #[test]
    fn case_insensitive() {
        assert!(key_is_sensitive("API_KEY"));
        assert!(key_is_sensitive("Password"));
        assert!(key_is_sensitive("SECRET"));
        assert!(key_is_sensitive("AuthToken"));
    }

    #[test]
    fn rejects_unrelated_keys() {
        assert!(!key_is_sensitive("port"));
        assert!(!key_is_sensitive("host"));
        assert!(!key_is_sensitive("model"));
        assert!(!key_is_sensitive("provider"));
        assert!(!key_is_sensitive("timeout_ms"));
        assert!(!key_is_sensitive("name"));
        assert!(!key_is_sensitive("description"));
        assert!(!key_is_sensitive("enabled"));
        assert!(!key_is_sensitive("path"));
    }

    /// Substring matching has a known small false-positive surface — keys
    /// like `max_tokens` and `bootstrap_max_tokens` contain "token" and
    /// will be flagged sensitive. This is the accepted failure-mode bias
    /// (#4254): better to over-encrypt/over-redact than to leak a real
    /// `auth_token`. Both paths only act on string values, so numeric
    /// fields named like this are not affected at runtime.
    #[test]
    fn substring_rule_flags_token_like_numeric_keys() {
        assert!(key_is_sensitive("max_tokens"));
        assert!(key_is_sensitive("bootstrap_max_tokens"));
        assert!(key_is_sensitive("context_tokens"));
    }

    #[test]
    fn empty_key_is_not_sensitive() {
        assert!(!key_is_sensitive(""));
    }

    /// Property test (#4254): the set of keys redacted on display must equal
    /// the set of keys encrypted at rest. Built by exercising both paths on
    /// the same input and comparing the resulting sensitivity decisions.
    #[test]
    fn redact_and_encrypt_agree_on_sensitive_keys() {
        use crate::encrypt::encrypt_sensitive_values;

        // Mix of obvious sensitive, subtle sensitive (prefixed/suffixed),
        // and clearly non-sensitive keys.
        let candidates = [
            "api_key",
            "apiKey",
            "openai_api_key",
            "anthropicApiKey",
            "token",
            "auth_token",
            "authToken",
            "refresh_token",
            "refreshToken",
            "id_token",
            "secret",
            "session_secret",
            "client_secret",
            "password",
            "admin_password",
            "signingKey",
            "signing_key",
            "jwtSigningKey",
            // not sensitive — must stay plaintext / visible in both paths
            "port",
            "host",
            "model",
            "provider",
            "timeout_ms",
            "max_tokens",
            "name",
            "description",
            "enabled",
        ];

        // What redact treats as sensitive (substring-matches the key).
        let mut redact_object = serde_json::Map::new();
        for k in candidates {
            redact_object.insert((*k).to_owned(), serde_json::Value::String("v".to_owned()));
        }
        let mut redact_value = serde_json::Value::Object(redact_object);
        crate::redact::redact_sensitive_keys_for_test(&mut redact_value);
        let redact_sensitive: std::collections::BTreeSet<&str> = candidates
            .iter()
            .filter(|k| {
                redact_value
                    .as_object()
                    .and_then(|m| m.get(**k))
                    .and_then(serde_json::Value::as_str)
                    == Some("***")
            })
            .copied()
            .collect();

        // What encrypt treats as sensitive (matches the key).
        let fixture_key: [u8; 32] = {
            let mut k = [0u8; 32];
            #[expect(
                clippy::cast_possible_truncation,
                reason = "INVARIANT: i is bounded by [u8; 32].len()=32; cast to u8 is exact"
            )]
            #[expect(
                clippy::as_conversions,
                reason = "INVARIANT: i is bounded by [u8; 32].len()=32; cast to u8 is exact"
            )]
            for (i, b) in k.iter_mut().enumerate() {
                *b = (i as u8).wrapping_mul(7).wrapping_add(42);
            }
            k
        };
        let mut encrypt_table = toml::value::Table::new();
        for k in candidates {
            encrypt_table.insert((*k).to_owned(), toml::Value::String("v".to_owned()));
        }
        let mut encrypt_value = toml::Value::Table(encrypt_table);
        #[expect(clippy::unwrap_used, reason = "test")]
        let _count = encrypt_sensitive_values(&mut encrypt_value, &fixture_key).unwrap();
        let encrypt_sensitive: std::collections::BTreeSet<&str> = candidates
            .iter()
            .filter(|k| {
                encrypt_value
                    .as_table()
                    .and_then(|t| t.get(**k))
                    .and_then(toml::Value::as_str)
                    .is_some_and(|s| s.starts_with("enc:"))
            })
            .copied()
            .collect();

        assert_eq!(
            redact_sensitive, encrypt_sensitive,
            "redact and encrypt must agree on which keys are sensitive (#4254)"
        );
    }
}
