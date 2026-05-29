//! Typed identifier newtypes for the envelope, factbase, and registry.
//!
//! Every identifier the agent or operator authors lands here as a typed
//! newtype whose construction enforces a charset and a length bound. Bare
//! `String`s are forbidden at the public surface; raw construction is a
//! `TryFrom<&str>` returning [`IdError`].
//!
//! The charset is intentionally narrow (lowercase ASCII letters, digits,
//! `_`, `-`) for `ComponentId`, `ThemeId`, `DataSourceId` because they map
//! to filesystem paths. `FactId`, `ClaimId`, `SheetName` accept a slightly
//! wider set so spec authors can write human-recognisable names.

use serde::{Deserialize, Serialize};

use crate::error::{EmptySnafu, IdError, InvalidCharSnafu, TooLongSnafu};

const FS_SAFE_MAX: usize = 64;
const HUMAN_NAME_MAX: usize = 128;

/// An identifier whose characters must be safe to embed in a filesystem path:
/// lowercase ASCII `a-z`, digits `0-9`, `_`, `-`. First character must be a
/// letter.
fn validate_fs_safe(input: &str, kind: &'static str) -> Result<(), IdError> {
    if input.is_empty() {
        return EmptySnafu { kind }.fail();
    }
    if input.len() > FS_SAFE_MAX {
        return TooLongSnafu {
            kind,
            input,
            got: input.len(),
            max: FS_SAFE_MAX,
        }
        .fail();
    }
    let mut chars = input.chars();
    let Some(first) = chars.next() else {
        return EmptySnafu { kind }.fail();
    };
    if !first.is_ascii_lowercase() {
        return InvalidCharSnafu {
            kind,
            input,
            ch: first,
            allowed: "first character must be lowercase ASCII letter (a-z)",
        }
        .fail();
    }
    for ch in chars {
        let ok = ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-';
        if !ok {
            return InvalidCharSnafu {
                kind,
                input,
                ch,
                allowed: "lowercase ASCII letters, digits, '_', '-'",
            }
            .fail();
        }
    }
    Ok(())
}

/// An identifier that may carry human-readable characters: ASCII letters,
/// digits, `_`, `-`, `.`, space. Disallows control characters and ASCII
/// quotes/brackets that would confuse error messages.
fn validate_human_name(input: &str, kind: &'static str) -> Result<(), IdError> {
    if input.is_empty() {
        return EmptySnafu { kind }.fail();
    }
    if input.len() > HUMAN_NAME_MAX {
        return TooLongSnafu {
            kind,
            input,
            got: input.len(),
            max: HUMAN_NAME_MAX,
        }
        .fail();
    }
    for ch in input.chars() {
        let ok = ch.is_ascii_alphanumeric()
            || matches!(ch, '_' | '-' | '.' | ' ' | ':' | '/')
            || (!ch.is_ascii() && !ch.is_control());
        if !ok {
            return InvalidCharSnafu {
                kind,
                input,
                ch,
                allowed: "alphanumerics, underscore, hyphen, dot, space, colon, slash, non-ASCII non-control",
            }
            .fail();
        }
    }
    Ok(())
}

macro_rules! fs_safe_id {
    ($name:ident, $kind:literal, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            #[doc = concat!("Construct a `", stringify!($name), "` from a string, validating charset and length.")]
            ///
            /// # Errors
            ///
            /// Returns [`IdError`] if the input is empty, too long, or contains
            /// disallowed characters.
            pub fn new(s: impl Into<String>) -> Result<Self, IdError> {
                let s = s.into();
                validate_fs_safe(&s, $kind)?;
                Ok(Self(s))
            }

            /// Borrow the inner string slice.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl TryFrom<String> for $name {
            type Error = IdError;
            fn try_from(s: String) -> Result<Self, Self::Error> {
                Self::new(s)
            }
        }

        impl TryFrom<&str> for $name {
            type Error = IdError;
            fn try_from(s: &str) -> Result<Self, Self::Error> {
                Self::new(s.to_owned())
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

macro_rules! human_id {
    ($name:ident, $kind:literal, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            #[doc = concat!("Construct a `", stringify!($name), "` from a string, validating charset and length.")]
            ///
            /// # Errors
            ///
            /// Returns [`IdError`] if the input is empty, too long, or contains
            /// disallowed characters.
            pub fn new(s: impl Into<String>) -> Result<Self, IdError> {
                let s = s.into();
                validate_human_name(&s, $kind)?;
                Ok(Self(s))
            }

            /// Borrow the inner string slice.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl TryFrom<String> for $name {
            type Error = IdError;
            fn try_from(s: String) -> Result<Self, Self::Error> {
                Self::new(s)
            }
        }

        impl TryFrom<&str> for $name {
            type Error = IdError;
            fn try_from(s: &str) -> Result<Self, Self::Error> {
                Self::new(s.to_owned())
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

fs_safe_id!(
    ComponentId,
    "component",
    "Identifier of a component pack discovered under `components/<id>/`."
);
fs_safe_id!(
    ThemeId,
    "theme",
    "Identifier of a theme registered with `poiesis-theme`."
);
fs_safe_id!(
    DataSourceId,
    "data_source",
    "Identifier of a configured [`crate::factbase::DataSource`] adapter."
);
human_id!(
    FactId,
    "fact",
    "Identifier of a [`crate::factbase::Fact`] entry in a `Factbase`."
);
human_id!(
    ClaimId,
    "claim",
    "Identifier of a [`crate::factbase::Claim`] entry in a `Factbase`."
);
human_id!(
    SheetName,
    "sheet",
    "Display name of a workbook sheet; constrained to safe characters."
);

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn component_id_accepts_canonical() {
        let id = ComponentId::new("stat_cards").expect("canonical id is valid");
        assert_eq!(id.as_str(), "stat_cards");
    }

    #[test]
    fn component_id_rejects_empty() {
        let err = ComponentId::new("").expect_err("empty id must reject");
        assert!(matches!(err, IdError::Empty { kind: "component" }));
    }

    #[test]
    fn component_id_rejects_uppercase() {
        let err = ComponentId::new("StatCards").expect_err("uppercase must reject");
        assert!(matches!(err, IdError::InvalidChar { ch: 'S', .. }));
    }

    #[test]
    fn component_id_rejects_leading_digit() {
        let err = ComponentId::new("2up").expect_err("leading digit must reject");
        assert!(matches!(err, IdError::InvalidChar { ch: '2', .. }));
    }

    #[test]
    fn component_id_rejects_overlong() {
        let s = "a".repeat(FS_SAFE_MAX + 1);
        let err = ComponentId::new(s).expect_err("overlong id must reject");
        assert!(matches!(err, IdError::TooLong { .. }));
    }

    #[test]
    fn theme_id_round_trips_via_serde() {
        let id = ThemeId::new("summus").expect("canonical theme is valid");
        let json = serde_json::to_string(&id).expect("serialize");
        assert_eq!(json, "\"summus\"");
        let back: ThemeId = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, id);
    }

    #[test]
    fn fact_id_accepts_human_chars() {
        let id = FactId::new("revenue.fy26.q1").expect("human name with dot is valid");
        assert_eq!(id.as_str(), "revenue.fy26.q1");
    }

    #[test]
    fn fact_id_rejects_quote() {
        let err = FactId::new("bad\"name").expect_err("quote must reject");
        assert!(matches!(err, IdError::InvalidChar { ch: '"', .. }));
    }

    #[test]
    fn sheet_name_accepts_space() {
        let n = SheetName::new("Q1 Receipts").expect("spaces allowed in sheet names");
        assert_eq!(n.as_str(), "Q1 Receipts");
    }

    #[test]
    fn serde_rejects_invalid_via_try_from() {
        let err: Result<ComponentId, _> = serde_json::from_str("\"BAD\"");
        assert!(err.is_err());
    }
}
