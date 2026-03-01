//! Newtype wrappers for domain identifiers.
//!
//! Every domain concept gets its own type. No raw `String` or `u64` for identifiers.
//! Construction validates. The type *is* the documentation.

use compact_str::CompactString;
use serde::{Deserialize, Serialize};
use std::fmt;

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
    pub fn parse(s: &str) -> Result<Self, IdError> {
        let ulid = s.parse::<ulid::Ulid>().map_err(|e| IdError::InvalidFormat {
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

// --- Validation ---

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

// --- Errors ---

/// Errors from identifier construction.
#[derive(Debug, Clone, PartialEq, Eq)]
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
}
