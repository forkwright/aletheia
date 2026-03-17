//! Newtype wrappers for mneme-local domain identifiers.
//!
//! These types prevent accidental mixing of ID kinds at compile time.
//! Cross-crate identifiers (`NousId`, `SessionId`) live in `koina::id`.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Maximum byte length for mneme-local IDs.
const MAX_ID_LEN: usize = 256;

macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Create a new identifier.
            ///
            /// # Errors
            /// Returns `IdValidationError` if the value is empty or exceeds
            /// the maximum length.
            pub fn new(id: impl Into<String>) -> Result<Self, IdValidationError> {
                let id = id.into();
                if id.is_empty() {
                    return Err(IdValidationError::Empty {
                        kind: stringify!($name),
                    });
                }
                if id.len() > MAX_ID_LEN {
                    return Err(IdValidationError::TooLong {
                        kind: stringify!($name),
                        max: MAX_ID_LEN,
                        actual: id.len(),
                    });
                }
                Ok(Self(id))
            }

            /// Create without validation: for internal row parsing where
            /// the ID was already validated on insert.
            #[must_use]
            // `#[expect]` cannot be used: the macro is invoked for multiple ID types; some
            // invocations have callers (fulfilling the lint) while others don't (unfulfilled
            // expectation). `#[allow]` applies to all invocations uniformly.
            #[allow(dead_code, reason = "used by row parsers and tests; not all ID types have production callers yet")]
            pub(crate) fn new_unchecked(id: impl Into<String>) -> Self {
                Self(id.into())
            }

            /// The underlying string value.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        /// Backward-compatibility conversion from `String`.
        impl From<String> for $name {
            fn from(s: String) -> Self {
                Self(s)
            }
        }

        /// Backward-compatibility conversion from `&str`.
        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.to_owned())
            }
        }
    };
}

define_id!(
    /// Unique identifier for a [`Fact`](crate::knowledge::Fact).
    FactId
);

define_id!(
    /// Unique identifier for an [`Entity`](crate::knowledge::Entity).
    EntityId
);

define_id!(
    /// Unique identifier for an [`EmbeddedChunk`](crate::knowledge::EmbeddedChunk).
    EmbeddingId
);

/// Validation errors for mneme-local identifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum IdValidationError {
    /// The identifier was empty.
    Empty { kind: &'static str },
    /// The identifier exceeded the maximum length.
    TooLong {
        kind: &'static str,
        max: usize,
        actual: usize,
    },
}

impl fmt::Display for IdValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty { kind } => write!(f, "{kind} cannot be empty"),
            Self::TooLong { kind, max, actual } => {
                write!(f, "{kind} too long: {actual} bytes (max {max})")
            }
        }
    }
}

impl std::error::Error for IdValidationError {}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn fact_id_and_entity_id_are_distinct_types() {
        let fact: FactId = "id-1".into();
        let entity: EntityId = "id-1".into();
        // Same string content, but different types: this is the point.
        assert_eq!(fact.as_str(), entity.as_str());
    }

    #[test]
    fn valid_id_creation() {
        assert!(FactId::new("fact-001").is_ok());
        assert!(EntityId::new("entity-abc").is_ok());
        assert!(EmbeddingId::new("emb-42").is_ok());
    }

    #[test]
    fn empty_id_rejected() {
        assert!(matches!(
            FactId::new(""),
            Err(IdValidationError::Empty { .. })
        ));
    }

    #[test]
    fn too_long_id_rejected() {
        let long = "x".repeat(MAX_ID_LEN + 1);
        assert!(matches!(
            FactId::new(long),
            Err(IdValidationError::TooLong { .. })
        ));
    }

    #[test]
    fn max_length_id_accepted() {
        let max = "x".repeat(MAX_ID_LEN);
        assert!(FactId::new(max).is_ok());
    }

    #[test]
    fn serde_roundtrip() {
        let id = FactId::new("fact-42").expect("valid");
        let json = serde_json::to_string(&id).expect("serialize");
        assert_eq!(json, r#""fact-42""#);
        let back: FactId = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(id, back);
    }

    #[test]
    fn display_shows_inner_value() {
        let id = EntityId::new("ent-1").expect("valid");
        assert_eq!(id.to_string(), "ent-1");
    }

    #[test]
    fn from_string_backward_compat() {
        let id: FactId = "test".into();
        assert_eq!(id.as_str(), "test");

        let id2: FactId = String::from("test2").into();
        assert_eq!(id2.as_str(), "test2");
    }

    #[test]
    fn as_ref_str() {
        let id = FactId::new("ref-test").expect("valid");
        let s: &str = id.as_ref();
        assert_eq!(s, "ref-test");
    }

    #[test]
    fn error_display() {
        let empty = IdValidationError::Empty { kind: "FactId" };
        assert_eq!(empty.to_string(), "FactId cannot be empty");

        let long = IdValidationError::TooLong {
            kind: "FactId",
            max: 256,
            actual: 300,
        };
        assert!(long.to_string().contains("300"));
    }
}
