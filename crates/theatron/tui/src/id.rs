//! Newtype wrappers for domain identifiers in the TUI layer.

use compact_str::CompactString;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NousId(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(String);

// WHY: Decimal u64 strings are at most 20 bytes (u64::MAX), always within
// CompactString's 24-byte inline limit. NousId (≤64 bytes), SessionId
// (26-byte ULID), ToolId (≤128 bytes), and PlanId (variable) exceed it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TurnId(CompactString);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ToolId(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PlanId(String);

macro_rules! impl_id {
    ($ty:ident) => {
        impl fmt::Display for $ty {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<String> for $ty {
            fn from(s: String) -> Self {
                Self(s.into())
            }
        }

        impl From<&str> for $ty {
            fn from(s: &str) -> Self {
                Self(s.into())
            }
        }

        impl AsRef<str> for $ty {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl std::ops::Deref for $ty {
            type Target = str;
            fn deref(&self) -> &str {
                &self.0
            }
        }

        impl PartialEq<str> for $ty {
            fn eq(&self, other: &str) -> bool {
                self.0 == other
            }
        }
    };
}

impl_id!(NousId);
impl_id!(SessionId);
impl_id!(TurnId);
impl_id!(ToolId);
impl_id!(PlanId);

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    #[test]
    fn serde_transparent_roundtrip() {
        let id = NousId::from("syn".to_string());
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, r#""syn""#);
        let back: NousId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn display_and_deref() {
        let id = SessionId::from("abc-123".to_string());
        assert_eq!(id.to_string(), "abc-123");
        assert_eq!(&*id, "abc-123");
        assert!(id == *"abc-123");
    }

    #[test]
    fn distinct_types_prevent_mixup() {
        let nous = NousId::from("agent".to_string());
        let session = SessionId::from("agent".to_string());
        // These are different types — can't accidentally compare or swap them
        assert_eq!(&*nous, &*session);
    }

    #[test]
    fn turn_id_serde_roundtrip() {
        let id = TurnId::from("42".to_string());
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, r#""42""#);
        let back: TurnId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn turn_id_max_value_fits_inline() {
        let max = TurnId::from(u64::MAX.to_string());
        assert_eq!(&*max, "18446744073709551615");
    }
}
