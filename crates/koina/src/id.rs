//! Newtype wrappers for domain identifiers.

use std::borrow::Borrow;
use std::fmt;

use compact_str::CompactString;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Generate a newtype ID wrapper around a string-like inner type.
///
/// Produces a transparent serde newtype with standard string-like trait
/// implementations. The inner type must implement `AsRef<str>`,
/// `From<String>`, `From<&str>`, and `Into<String>`.
///
/// # Generated API
///
/// - **Derives:** `Debug`, `Clone`, `PartialEq`, `Eq`, `Hash`, `Serialize`, `Deserialize`
/// - **Traits:** `Display`, `FromStr`, `AsRef<str>`, `Borrow<str>`, `Deref<Target=str>`,
///   `From<String>`, `From<&str>`, `From<T> for String`, `PartialEq<str>`
/// - **Methods:** `new()`, `into_inner()`, `as_str()`
///
/// # Examples
///
/// ```
/// use aletheia_koina::newtype_id;
///
/// newtype_id!(
///     /// A widget identifier.
///     pub struct WidgetId(String)
/// );
///
/// let id = WidgetId::new("w-1");
/// assert_eq!(id.as_str(), "w-1");
/// assert_eq!(id.to_string(), "w-1");
/// let back: String = id.into_inner();
/// assert_eq!(back, "w-1");
/// ```
#[macro_export]
macro_rules! newtype_id {
    ($(#[$meta:meta])* $vis:vis struct $name:ident($inner:ty)) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, PartialEq, Eq, Hash,
            ::serde::Serialize, ::serde::Deserialize,
        )]
        #[serde(transparent)]
        $vis struct $name($inner);

        impl $name {
            /// Create a new identifier.
            #[must_use]
            $vis fn new(id: impl Into<$inner>) -> Self {
                Self(id.into())
            }

            /// Consume the wrapper and return the inner value.
            #[must_use]
            $vis fn into_inner(self) -> $inner {
                self.0.into()
            }

            /// The underlying string value.
            #[must_use]
            $vis fn as_str(&self) -> &str {
                self.0.as_ref()
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(self.0.as_ref())
            }
        }

        impl ::std::str::FromStr for $name {
            type Err = ::std::convert::Infallible;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Ok(Self(s.into()))
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.0.as_ref()
            }
        }

        impl ::std::borrow::Borrow<str> for $name {
            fn borrow(&self) -> &str {
                self.0.as_ref()
            }
        }

        impl ::std::ops::Deref for $name {
            type Target = str;

            fn deref(&self) -> &str {
                self.0.as_ref()
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                Self(s.into())
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.into())
            }
        }

        impl From<$name> for String {
            fn from(id: $name) -> Self {
                id.0.into()
            }
        }

        impl PartialEq<str> for $name {
            fn eq(&self, other: &str) -> bool {
                self.as_ref() == other
            }
        }
    };
}

/// A nous (agent) identifier. Lowercase alphanumeric + hyphens, 1-64 chars.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct NousId(CompactString);

impl NousId {
    /// Create a new `NousId`, validating the format.
    ///
    /// # Errors
    /// Returns an error if the ID is empty, exceeds 64 characters,
    /// or contains characters other than lowercase alphanumeric and hyphens.
    pub fn new(id: impl Into<CompactString>) -> Result<Self, IdError> {
        let id = id.into();
        validate_id(&id, "NousId")?;
        Ok(Self(id))
    }

    /// The underlying string value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NousId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for NousId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for NousId {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for NousId {
    type Error = IdError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::new(s)
    }
}

impl From<NousId> for String {
    fn from(id: NousId) -> Self {
        id.0.into()
    }
}

/// A session identifier. UUID v4-based, cryptographically random (128-bit).
///
/// WHY: ULID uses only 80 bits of randomness; UUID v4 provides 122 bits,
/// eliminating any practical guessability risk for session tokens.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(Uuid);

impl SessionId {
    /// Generate a new session ID using UUID v4 (128-bit random).
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Parse from a UUID string (hyphenated format).
    ///
    /// # Errors
    /// Returns an error if the string is not a valid UUID.
    pub fn parse(s: &str) -> Result<Self, IdError> {
        Uuid::parse_str(s)
            .map(Self)
            .map_err(|e| IdError::InvalidFormat {
                kind: "SessionId",
                value: s.to_owned(),
                reason: e.to_string(),
            })
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.hyphenated())
    }
}

/// A turn identifier. Sequential within a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TurnId(u64);

impl TurnId {
    /// Create a new turn ID.
    #[must_use]
    pub fn new(n: u64) -> Self {
        Self(n)
    }

    /// The underlying numeric value.
    #[must_use]
    pub fn as_u64(self) -> u64 {
        self.0
    }

    /// Increment to next turn.
    #[must_use]
    pub fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

impl From<u64> for TurnId {
    fn from(n: u64) -> Self {
        Self(n)
    }
}

impl From<TurnId> for u64 {
    fn from(id: TurnId) -> Self {
        id.0
    }
}

impl fmt::Display for TurnId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A tool name. Validated to be non-empty and contain only safe characters.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ToolName(CompactString);

impl ToolName {
    /// Create a new tool name.
    ///
    /// # Errors
    /// Returns an error if the name is empty, exceeds 128 characters,
    /// or contains characters other than alphanumeric, hyphens, and underscores.
    pub fn new(name: impl Into<CompactString>) -> Result<Self, IdError> {
        let name = name.into();
        if name.is_empty() {
            return Err(IdError::Empty { kind: "ToolName" });
        }
        if name.len() > 128 {
            return Err(IdError::TooLong {
                kind: "ToolName",
                max: 128,
                actual: name.len(),
            });
        }
        if !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(IdError::InvalidFormat {
                kind: "ToolName",
                value: name.to_string(),
                reason: "must contain only alphanumeric, hyphens, and underscores".to_owned(),
            });
        }
        Ok(Self(name))
    }

    /// The underlying string value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ToolName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for ToolName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for ToolName {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ToolName {
    type Error = IdError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::new(s)
    }
}

impl From<ToolName> for String {
    fn from(name: ToolName) -> Self {
        name.0.into()
    }
}

fn validate_id(id: &str, kind: &'static str) -> Result<(), IdError> {
    if id.is_empty() {
        return Err(IdError::Empty { kind });
    }
    if id.len() > 64 {
        return Err(IdError::TooLong {
            kind,
            max: 64,
            actual: id.len(),
        });
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(IdError::InvalidFormat {
            kind,
            value: id.to_owned(),
            reason: "must contain only lowercase alphanumeric and hyphens".to_owned(),
        });
    }
    Ok(())
}

/// Errors from identifier construction.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum IdError {
    /// The identifier was empty.
    Empty {
        /// The identifier type name (e.g. "NousId").
        kind: &'static str,
    },
    /// The identifier exceeded the maximum length.
    TooLong {
        /// The identifier type name (e.g. "NousId").
        kind: &'static str,
        /// Maximum allowed length.
        max: usize,
        /// Actual length that was provided.
        actual: usize,
    },
    /// The identifier contained invalid characters or format.
    InvalidFormat {
        /// The identifier type name (e.g. "NousId").
        kind: &'static str,
        /// The value that failed validation.
        value: String,
        /// Description of why the format is invalid.
        reason: String,
    },
}

impl fmt::Display for IdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty { kind } => write!(f, "{kind} cannot be empty"),
            Self::TooLong { kind, max, actual } => {
                write!(f, "{kind} too long: {actual} chars (max {max})")
            }
            Self::InvalidFormat {
                kind,
                value,
                reason,
            } => write!(f, "invalid {kind} '{value}': {reason}"),
        }
    }
}

impl std::error::Error for IdError {}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn valid_nous_id() {
        assert!(NousId::new("syn").is_ok());
        assert!(NousId::new("demiurge").is_ok());
        assert!(NousId::new("worker-1").is_ok());
    }

    #[test]
    fn invalid_nous_id_empty() {
        assert!(matches!(NousId::new(""), Err(IdError::Empty { .. })));
    }

    #[test]
    fn invalid_nous_id_uppercase() {
        assert!(matches!(
            NousId::new("Syn"),
            Err(IdError::InvalidFormat { .. })
        ));
    }

    #[test]
    fn invalid_nous_id_too_long() {
        let long = "a".repeat(65);
        assert!(matches!(NousId::new(long), Err(IdError::TooLong { .. })));
    }

    #[test]
    fn nous_id_display() {
        let id = NousId::new("syn").unwrap();
        assert_eq!(id.to_string(), "syn");
        assert_eq!(id.as_str(), "syn");
    }

    #[test]
    fn nous_id_serde_roundtrip() {
        let id = NousId::new("syn").unwrap();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, r#""syn""#);
        let back: NousId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn session_id_unique() {
        let a = SessionId::new();
        let b = SessionId::new();
        assert_ne!(a, b);
    }

    #[test]
    fn session_id_parse_roundtrip() {
        let id = SessionId::new();
        let s = id.to_string();
        let back = SessionId::parse(&s).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn turn_id_ordering() {
        let a = TurnId::new(1);
        let b = TurnId::new(2);
        assert!(a < b);
        assert_eq!(a.next(), b);
    }

    #[test]
    fn valid_tool_name() {
        assert!(ToolName::new("exec").is_ok());
        assert!(ToolName::new("web_search").is_ok());
        assert!(ToolName::new("sessions-ask").is_ok());
    }

    #[test]
    fn invalid_tool_name_spaces() {
        assert!(matches!(
            ToolName::new("my tool"),
            Err(IdError::InvalidFormat { .. })
        ));
    }

    #[test]
    fn tool_name_serde_roundtrip() {
        let name = ToolName::new("exec").unwrap();
        let json = serde_json::to_string(&name).unwrap();
        let back: ToolName = serde_json::from_str(&json).unwrap();
        assert_eq!(name, back);
    }

    #[test]
    fn nous_id_max_length_accepted() {
        let max = "a".repeat(64);
        assert!(NousId::new(max).is_ok());
    }

    #[test]
    fn nous_id_leading_hyphen() {
        assert!(NousId::new("-syn").is_ok());
    }

    #[test]
    fn nous_id_digits_only() {
        assert!(NousId::new("123").is_ok());
    }

    #[test]
    fn nous_id_special_chars_rejected() {
        assert!(matches!(
            NousId::new("syn_1"),
            Err(IdError::InvalidFormat { .. })
        ));
        assert!(matches!(
            NousId::new("syn.1"),
            Err(IdError::InvalidFormat { .. })
        ));
        assert!(matches!(
            NousId::new("syn 1"),
            Err(IdError::InvalidFormat { .. })
        ));
    }

    #[test]
    fn tool_name_max_length_accepted() {
        let max = "a".repeat(128);
        assert!(ToolName::new(max).is_ok());
    }

    #[test]
    fn tool_name_empty_rejected() {
        assert!(matches!(ToolName::new(""), Err(IdError::Empty { .. })));
    }

    #[test]
    fn tool_name_too_long_rejected() {
        let long = "a".repeat(129);
        assert!(matches!(ToolName::new(long), Err(IdError::TooLong { .. })));
    }

    #[test]
    fn tool_name_only_hyphens_underscores() {
        assert!(ToolName::new("--__--").is_ok());
    }

    #[test]
    fn session_id_parse_invalid() {
        assert!(SessionId::parse("").is_err());
        assert!(SessionId::parse("not-a-uuid").is_err());
        assert!(SessionId::parse("too-short").is_err());
    }

    #[test]
    fn session_id_display_is_uuid_format() {
        let id = SessionId::new();
        let s = id.to_string();
        // UUID hyphenated format: xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx (36 chars)
        assert_eq!(s.len(), 36, "session ID must be 36-char hyphenated UUID");
        assert!(
            s.chars().all(|c| c.is_ascii_hexdigit() || c == '-'),
            "session ID must be hex and hyphens"
        );
    }

    #[test]
    fn turn_id_zero() {
        let t = TurnId::new(0);
        assert_eq!(t.as_u64(), 0);
        assert_eq!(t.next(), TurnId::new(1));
    }

    #[test]
    fn turn_id_display() {
        assert_eq!(TurnId::new(42).to_string(), "42");
        assert_eq!(TurnId::new(0).to_string(), "0");
    }

    #[test]
    fn nous_id_as_ref_and_borrow() {
        let id = NousId::new("syn").unwrap();
        let s: &str = id.as_ref();
        assert_eq!(s, "syn");
        let b: &str = id.borrow();
        assert_eq!(b, "syn");
    }

    #[test]
    fn nous_id_borrow_hashmap_lookup() {
        let id = NousId::new("syn").unwrap();
        let mut map = std::collections::HashMap::new();
        map.insert(id, 42);
        assert_eq!(map.get("syn"), Some(&42));
    }

    #[test]
    fn session_id_parse_roundtrip_uuid() {
        let id = SessionId::new();
        let s = id.to_string();
        let back = SessionId::parse(&s).unwrap();
        assert_eq!(id, back, "parse-roundtrip must be identity");
    }

    #[test]
    fn turn_id_from_u64_roundtrip() {
        let n: u64 = 42;
        let id = TurnId::from(n);
        let back: u64 = id.into();
        assert_eq!(n, back);
    }

    #[test]
    fn turn_id_from_matches_new() {
        assert_eq!(TurnId::from(7), TurnId::new(7));
    }

    #[test]
    fn tool_name_as_ref_and_borrow() {
        let name = ToolName::new("exec").unwrap();
        let s: &str = name.as_ref();
        assert_eq!(s, "exec");
        let b: &str = name.borrow();
        assert_eq!(b, "exec");
    }

    #[test]
    fn tool_name_borrow_hashmap_lookup() {
        let name = ToolName::new("exec").unwrap();
        let mut map = std::collections::HashMap::new();
        map.insert(name, 99);
        assert_eq!(map.get("exec"), Some(&99));
    }

    #[test]
    fn id_error_display_formats() {
        let empty = IdError::Empty { kind: "NousId" };
        assert_eq!(empty.to_string(), "NousId cannot be empty");

        let long = IdError::TooLong {
            kind: "NousId",
            max: 64,
            actual: 100,
        };
        assert!(long.to_string().contains("100"));

        let fmt = IdError::InvalidFormat {
            kind: "NousId",
            value: "Bad".to_owned(),
            reason: "uppercase".to_owned(),
        };
        assert!(fmt.to_string().contains("Bad"));
    }

    mod newtype_id_macro {
        newtype_id!(
            /// Test ID using String inner type.
            pub struct TestStringId(String)
        );

        newtype_id!(
            /// Test ID using `CompactString` inner type.
            pub struct TestCompactId(compact_str::CompactString)
        );

        #[test]
        fn new_and_as_str() {
            let id = TestStringId::new("abc");
            assert_eq!(id.as_str(), "abc");
        }

        #[test]
        fn into_inner_returns_owned() {
            let id = TestStringId::new("abc");
            let inner: String = id.into_inner();
            assert_eq!(inner, "abc");
        }

        #[test]
        fn display_writes_inner() {
            let id = TestStringId::new("x-1");
            assert_eq!(id.to_string(), "x-1");
        }

        #[test]
        fn from_str_infallible() {
            let id: TestStringId = "hello".parse().unwrap();
            assert_eq!(id.as_str(), "hello");
        }

        #[test]
        fn from_string_and_str() {
            let a = TestStringId::from("abc");
            let b = TestStringId::from(String::from("abc"));
            assert_eq!(a, b);
        }

        #[test]
        fn into_string() {
            let id = TestStringId::new("val");
            let s: String = id.into();
            assert_eq!(s, "val");
        }

        #[test]
        fn deref_to_str() {
            let id = TestStringId::new("deref");
            assert_eq!(&*id, "deref");
            assert!(id.starts_with("de"));
        }

        #[test]
        fn partial_eq_str() {
            let id = TestStringId::new("cmp");
            assert!(id == *"cmp");
        }

        #[test]
        fn borrow_hashmap_lookup() {
            let id = TestStringId::new("key");
            let mut map = std::collections::HashMap::new();
            map.insert(id, 1);
            assert_eq!(map.get("key"), Some(&1));
        }

        #[test]
        fn serde_roundtrip() {
            let id = TestStringId::new("serde-test");
            let json = serde_json::to_string(&id).unwrap();
            assert_eq!(json, r#""serde-test""#);
            let back: TestStringId = serde_json::from_str(&json).unwrap();
            assert_eq!(id, back);
        }

        #[test]
        fn compact_string_variant_works() {
            let id = TestCompactId::new("compact");
            assert_eq!(id.as_str(), "compact");
            assert_eq!(id.to_string(), "compact");

            let json = serde_json::to_string(&id).unwrap();
            assert_eq!(json, r#""compact""#);
            let back: TestCompactId = serde_json::from_str(&json).unwrap();
            assert_eq!(id, back);
        }

        #[test]
        fn distinct_types_not_interchangeable() {
            let a = TestStringId::new("x");
            let b = TestCompactId::new("x");
            assert_eq!(a.as_str(), b.as_str());
            // WHY: a == b would not compile: different types
        }
    }
}
