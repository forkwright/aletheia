//! Newtype wrappers for domain identifiers shared across all frontends.

use std::borrow::Borrow;
use std::fmt;

use serde::{Deserialize, Serialize};

koina::newtype_id!(
    /// Agent (nous) identifier.
    pub struct ApiNousId(String) // kanon:ignore RUST/pub-visibility
);

koina::newtype_id!(
    /// Session identifier.
    pub struct ApiSessionId(String) // kanon:ignore RUST/pub-visibility
);

koina::newtype_id!(
    /// Request identifier, stamped by pylon on stream-start payloads.
    pub struct RequestId(String) // kanon:ignore RUST/pub-visibility
);

/// Turn identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TurnId(String);

koina::newtype_id!(
    /// Tool call identifier.
    pub struct ToolId(String) // kanon:ignore RUST/pub-visibility
);

/// Plan identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PlanId(String);

/// Git commit SHA.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GitSha(String);

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

        impl From<$ty> for String {
            fn from(id: $ty) -> Self {
                id.0.into()
            }
        }

        impl Borrow<str> for $ty {
            fn borrow(&self) -> &str {
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

impl_id!(TurnId);
impl_id!(PlanId);
impl_id!(GitSha);

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    #[test]
    fn serde_transparent_roundtrip() {
        let id = ApiNousId::from("syn".to_string());
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, r#""syn""#);
        let back: ApiNousId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn display_and_deref() {
        let id = ApiSessionId::from("abc-123".to_string());
        assert_eq!(id.to_string(), "abc-123");
        assert_eq!(&*id, "abc-123");
        assert_eq!(id, *"abc-123");
    }

    #[test]
    fn distinct_types_prevent_mixup() {
        let nous = ApiNousId::from("agent".to_string());
        let session = ApiSessionId::from("agent".to_string());
        // These are different types: can't accidentally compare or swap them
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

    #[test]
    fn nous_id_into_string() {
        let id = ApiNousId::from("syn");
        let s: String = id.into();
        assert_eq!(s, "syn");
    }

    #[test]
    fn session_id_into_string() {
        let id = ApiSessionId::from("sess-1");
        let s: String = id.into();
        assert_eq!(s, "sess-1");
    }

    #[test]
    fn tool_id_into_string() {
        let id = ToolId::from("t1");
        let s: String = id.into();
        assert_eq!(s, "t1");
    }

    #[test]
    fn plan_id_into_string() {
        let id = PlanId::from("p1");
        let s: String = id.into();
        assert_eq!(s, "p1");
    }

    #[test]
    fn borrow_hashmap_lookup() {
        let id = ApiNousId::from("syn");
        let mut map = std::collections::HashMap::new();
        map.insert(id, 42);
        assert_eq!(map.get("syn"), Some(&42));
    }

    #[test]
    fn as_ref_returns_inner() {
        let id = ToolId::from("exec");
        let s: &str = id.as_ref();
        assert_eq!(s, "exec");
    }

    // --- TurnId coverage ---

    #[test]
    fn turn_id_deref_str() {
        let id = TurnId::from("7");
        assert_eq!(&*id, "7", "Deref<Target=str> must expose inner value");
    }

    #[test]
    fn turn_id_as_ref_str() {
        let id = TurnId::from("99");
        let s: &str = id.as_ref();
        assert_eq!(s, "99");
    }

    #[test]
    fn turn_id_partial_eq_str() {
        let id = TurnId::from("turn-42");
        assert!(id == *"turn-42", "TurnId must PartialEq<str>");
    }

    #[test]
    fn turn_id_clone_equals_original() {
        let id = TurnId::from("abc");
        let clone = id.clone();
        assert_eq!(id, clone, "clone must equal original");
    }

    #[test]
    fn turn_id_into_string() {
        let id = TurnId::from("5");
        let s: String = id.into();
        assert_eq!(s, "5");
    }

    // --- PlanId coverage ---

    #[test]
    fn plan_id_deref_str() {
        let id = PlanId::from("plan-alpha");
        assert_eq!(
            &*id, "plan-alpha",
            "Deref<Target=str> must expose inner value"
        );
    }

    #[test]
    fn plan_id_display() {
        let id = PlanId::from("plan-1");
        assert_eq!(id.to_string(), "plan-1");
    }

    #[test]
    fn plan_id_partial_eq_str() {
        let id = PlanId::from("plan-beta");
        assert!(id == *"plan-beta", "PlanId must PartialEq<str>");
    }

    #[test]
    fn plan_id_borrow_hashmap_lookup() {
        let id = PlanId::from("plan-x");
        let mut map = std::collections::HashMap::new();
        map.insert(id, 99u32);
        assert_eq!(
            map.get("plan-x"),
            Some(&99u32),
            "Borrow<str> must allow &str HashMap lookup"
        );
    }

    // --- newtype_id! `new()` validation (#6608) ---

    #[test]
    fn nous_id_new_rejects_empty() {
        assert!(ApiNousId::new("").is_err());
    }

    #[test]
    fn nous_id_new_rejects_oversized() {
        let oversized = "a".repeat(koina::id::NEWTYPE_ID_MAX_LEN + 1);
        assert!(ApiNousId::new(oversized).is_err());
    }

    #[test]
    fn nous_id_new_rejects_control_character() {
        assert!(ApiNousId::new("bad\u{0}id").is_err());
    }

    #[test]
    fn nous_id_new_accepts_valid() {
        assert_eq!(ApiNousId::new("syn").unwrap().as_str(), "syn");
    }

    #[test]
    fn session_id_new_rejects_empty() {
        assert!(ApiSessionId::new("").is_err());
    }

    #[test]
    fn tool_id_new_rejects_empty() {
        assert!(ToolId::new("").is_err());
    }

    #[test]
    fn request_id_new_rejects_empty() {
        assert!(RequestId::new("").is_err());
    }
}
