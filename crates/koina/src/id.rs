//! Newtype wrappers for domain identifiers.

use std::fmt;

use compact_str::CompactString;
use serde::{Deserialize, Serialize};
use std::borrow::Borrow;

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
    #[must_use]
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

/// A session identifier. ULID-based, globally unique.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(ulid::Ulid);

impl SessionId {
    /// Generate a new session ID.
    #[must_use]
    pub fn new() -> Self {
        Self(ulid::Ulid::new())
    }

    /// Create from an existing ULID.
    #[must_use]
    pub fn from_ulid(ulid: ulid::Ulid) -> Self {
        Self(ulid)
    }

    /// Parse from a ULID string.
    ///
    /// # Errors
    /// Returns an error if the string is not a valid ULID.
    #[must_use]
    pub fn parse(s: &str) -> Result<Self, IdError> {
        let ulid = s
            .parse::<ulid::Ulid>()
            .map_err(|e| IdError::InvalidFormat {
                kind: "SessionId",
                value: s.to_owned(),
                reason: e.to_string(),
            })?;
        Ok(Self(ulid))
    }

    /// The underlying ULID.
    #[must_use]
    pub fn as_ulid(&self) -> ulid::Ulid {
        self.0
    }
}

impl From<ulid::Ulid> for SessionId {
    fn from(ulid: ulid::Ulid) -> Self {
        Self(ulid)
    }
}

impl From<SessionId> for ulid::Ulid {
    fn from(id: SessionId) -> Self {
        id.0
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
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
    #[must_use]
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
    Empty { kind: &'static str },
    /// The identifier exceeded the maximum length.
    TooLong {
        kind: &'static str,
        max: usize,
        actual: usize,
    },
    /// The identifier contained invalid characters or format.
    InvalidFormat {
        kind: &'static str,
        value: String,
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
        assert!(SessionId::parse("not-a-ulid").is_err());
        assert!(SessionId::parse("too-short").is_err());
    }

    #[test]
    fn session_id_display_is_ulid_format() {
        let id = SessionId::new();
        let s = id.to_string();
        assert_eq!(s.len(), 26);
        assert!(s.chars().all(|c| c.is_ascii_alphanumeric()));
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
    fn session_id_from_ulid_roundtrip() {
        let ulid = ulid::Ulid::new();
        let id = SessionId::from(ulid);
        let back: ulid::Ulid = id.into();
        assert_eq!(ulid, back);
    }

    #[test]
    fn session_id_from_ulid_matches_from_ulid_method() {
        let ulid = ulid::Ulid::new();
        let from_trait = SessionId::from(ulid);
        let from_method = SessionId::from_ulid(ulid);
        assert_eq!(from_trait, from_method);
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
}
