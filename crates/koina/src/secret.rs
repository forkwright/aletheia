//! Secret string newtype that prevents accidental leakage of sensitive values.

use std::fmt;

use serde::de::{Deserialize, Deserializer};
use serde::ser::{Serialize, Serializer};
use zeroize::Zeroize;

const REDACTED: &str = "[REDACTED]";

/// A string holding a secret value (API key, token, password).
///
/// - `Debug` and `Display` print `[REDACTED]` instead of the value.
/// - `Serialize` outputs `"[REDACTED]"` to prevent accidental logging via JSON.
/// - `Deserialize` accepts the actual string value normally.
/// - The backing memory is zeroed on drop via [`zeroize`].
/// - Use [`.expose_secret()`](Self::expose_secret) for intentional access.
pub struct SecretString { // kanon:ignore RUST/pub-visibility
    inner: String,
}

impl SecretString {
    /// Access the secret value. Call sites using this method are the audit
    /// surface for secret exposure.
    #[must_use]
    pub fn expose_secret(&self) -> &str { // kanon:ignore RUST/pub-visibility
        &self.inner
    }
}

impl Clone for SecretString {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl PartialEq for SecretString {
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

impl Eq for SecretString {}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

impl fmt::Display for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        self.inner.zeroize();
    }
}

impl From<String> for SecretString {
    fn from(s: String) -> Self {
        Self { inner: s }
    }
}

impl From<&str> for SecretString {
    fn from(s: &str) -> Self {
        Self {
            inner: s.to_owned(),
        }
    }
}

impl Serialize for SecretString {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(REDACTED)
    }
}

impl<'de> Deserialize<'de> for SecretString {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Ok(Self::from(s))
    }
}

/// Deserialize an `Option<SecretString>` that treats empty strings as `None`.
///
/// Useful for config fields where an empty value means "not set."
pub fn deserialize_option_secret_non_empty<'de, D>( // kanon:ignore RUST/pub-visibility
    deserializer: D,
) -> Result<Option<SecretString>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt: Option<String> = Option::deserialize(deserializer)?;
    Ok(opt.filter(|s| !s.is_empty()).map(SecretString::from))
}

/// Serialize an `Option<SecretString>` that uses the default serde behavior
/// (serialize the field as `[REDACTED]` if Some, skip if None when combined
/// with `#[serde(skip_serializing_if = "Option::is_none")]`).
///
/// This exists for symmetry with [`deserialize_option_secret_non_empty`].
pub fn serialize_option_secret_redacted<S>( // kanon:ignore RUST/pub-visibility
    value: &Option<SecretString>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match value {
        Some(_) => serializer.serialize_str(REDACTED),
        None => serializer.serialize_none(),
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts_value() {
        let s = SecretString::from("super-secret-key");
        let debug = format!("{s:?}");
        assert_eq!(debug, "[REDACTED]");
        assert!(!debug.contains("super-secret-key"));
    }

    #[test]
    fn display_redacts_value() {
        let s = SecretString::from("super-secret-key");
        let display = format!("{s}");
        assert_eq!(display, "[REDACTED]");
        assert!(!display.contains("super-secret-key"));
    }

    #[test]
    fn expose_secret_returns_inner_value() {
        let s = SecretString::from("my-api-key");
        assert_eq!(s.expose_secret(), "my-api-key");
    }

    #[test]
    fn clone_preserves_value() {
        let s = SecretString::from("key");
        let cloned = s.clone();
        assert_eq!(s.expose_secret(), cloned.expose_secret());
    }

    #[test]
    fn equality_compares_inner_values() {
        let a = SecretString::from("same");
        let b = SecretString::from("same");
        let c = SecretString::from("different");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn from_string_and_str() {
        let from_str = SecretString::from("hello");
        let from_string = SecretString::from(String::from("hello"));
        assert_eq!(from_str, from_string);
    }

    #[test]
    fn serialize_produces_redacted() {
        let s = SecretString::from("actual-secret");
        let json = serde_json::to_string(&s).expect("serialization should not fail");
        assert_eq!(json, r#""[REDACTED]""#);
        assert!(!json.contains("actual-secret"));
    }

    #[test]
    fn deserialize_accepts_actual_value() {
        let json = r#""my-secret-token""#;
        let s: SecretString = serde_json::from_str(json).expect("deserialization should not fail");
        assert_eq!(s.expose_secret(), "my-secret-token");
    }

    #[test]
    fn serialize_deserialize_option_some() {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct Wrapper {
            #[serde(skip_serializing_if = "Option::is_none")]
            token: Option<SecretString>,
        }
        let w = Wrapper {
            token: Some(SecretString::from("tok")),
        };
        let json = serde_json::to_string(&w).expect("serialization should not fail");
        assert!(json.contains("[REDACTED]"));
        // WHY: json key is "token" which contains "tok", check the value only
        assert!(
            !json.contains(r#""tok""#),
            "actual secret value should not appear in serialized output"
        );

        let back: Wrapper =
            serde_json::from_str(r#"{"token": "tok"}"#).expect("deserialization should not fail");
        assert_eq!(
            back.token.as_ref().map(SecretString::expose_secret),
            Some("tok")
        );
    }

    #[test]
    fn serialize_deserialize_option_none() {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct Wrapper {
            #[serde(skip_serializing_if = "Option::is_none")]
            token: Option<SecretString>,
        }
        let w = Wrapper { token: None };
        let json = serde_json::to_string(&w).expect("serialization should not fail");
        assert_eq!(json, "{}");
    }

    #[test]
    fn drop_zeroes_via_zeroize() {
        // WHY: We cannot safely inspect freed memory, so we verify the
        // zeroize contract indirectly by checking that the String is empty
        // after zeroize() is called (which Drop delegates to).
        let mut s = String::from("SECRET");
        assert!(!s.is_empty(), "precondition: string is non-empty");
        zeroize::Zeroize::zeroize(&mut s);
        assert!(s.is_empty(), "zeroize should clear the String");
    }
}
