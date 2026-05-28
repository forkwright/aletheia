use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use snafu::Snafu;

/// A theme identifier. Construction is the parse boundary: a `ThemeId` only
/// exists if it satisfies the registry-friendly identifier shape.
///
/// The valid alphabet is `[a-z0-9_-]` with a leading `[a-z]`. The length window
/// is 2..=64 characters. These bounds are the maximum overlap of:
///
/// - filesystem-safe (no separators, no shell metacharacters, no case folding),
/// - CSS-custom-property-safe (drives `--theme-<id>` prefixes),
/// - URL-safe (so a theme name flows through links without escaping),
/// - registry-friendly (the same shape the model registry will adopt in
///   [B-001](https://github.com/forkwright/aletheia)).
///
/// This is the parse-don't-validate boundary B-002 names: once a `ThemeId`
/// exists, every downstream surface (registry lookup, CSS emission, OOXML
/// theme-name attribute, doc-vars key prefix) can trust the value.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct ThemeId(String);

impl ThemeId {
    /// Minimum permitted length, inclusive.
    pub const MIN_LEN: usize = 2;
    /// Maximum permitted length, inclusive.
    pub const MAX_LEN: usize = 64;

    /// Parse a `&str` into a `ThemeId`. Returns [`InvalidThemeId`] with a
    /// reason on the first violation; the input is not mutated on failure.
    ///
    /// Time: O(n) in the candidate length. Space: O(n) for the owned
    /// representation on success, O(1) on failure.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidThemeId`] if the candidate is empty, outside the
    /// length window, begins with a non-`[a-z]` character, or contains any
    /// character outside `[a-z0-9_-]`.
    pub fn parse(candidate: &str) -> Result<Self, InvalidThemeId> {
        if candidate.is_empty() {
            return Err(InvalidThemeId::Empty);
        }
        let len = candidate.len();
        if !(Self::MIN_LEN..=Self::MAX_LEN).contains(&len) {
            return Err(InvalidThemeId::Length {
                len,
                min: Self::MIN_LEN,
                max: Self::MAX_LEN,
            });
        }
        let mut chars = candidate.chars();
        match chars.next() {
            Some(c) if c.is_ascii_lowercase() => {}
            Some(c) => return Err(InvalidThemeId::Leading { found: c }),
            None => return Err(InvalidThemeId::Empty),
        }
        for c in chars {
            if !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-') {
                return Err(InvalidThemeId::Character { found: c });
            }
        }
        Ok(Self(candidate.to_owned()))
    }

    /// Borrow the underlying string. Always satisfies [`ThemeId::parse`].
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the newtype, returning the inner string.
    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for ThemeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for ThemeId {
    type Err = InvalidThemeId;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl AsRef<str> for ThemeId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ThemeId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

/// Reason a candidate string was rejected by [`ThemeId::parse`].
#[derive(Debug, Snafu, PartialEq, Eq)]
#[non_exhaustive]
pub enum InvalidThemeId {
    /// The candidate was empty.
    #[snafu(display("theme id is empty"))]
    Empty,
    /// The candidate length was outside [[`ThemeId::MIN_LEN`], [`ThemeId::MAX_LEN`]].
    #[snafu(display("theme id length {len} is outside [{min}, {max}]"))]
    Length {
        /// Length of the rejected candidate.
        len: usize,
        /// Minimum permitted length.
        min: usize,
        /// Maximum permitted length.
        max: usize,
    },
    /// The first character was not `[a-z]`.
    #[snafu(display("theme id must begin with [a-z]; found {found:?}"))]
    Leading {
        /// The disallowed leading character.
        found: char,
    },
    /// A character outside `[a-z0-9_-]` appeared after position 0.
    #[snafu(display("theme id may only contain [a-z0-9_-]; found {found:?}"))]
    Character {
        /// The disallowed character.
        found: char,
    },
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_minimal_id() {
        let id = ThemeId::parse("aa").expect("two-char lowercase ascii must parse");
        assert_eq!(id.as_str(), "aa");
    }

    #[test]
    fn parse_accepts_summus() {
        let id = ThemeId::parse("summus").expect("seed theme name must parse");
        assert_eq!(id.as_str(), "summus");
    }

    #[test]
    fn parse_accepts_hyphen_and_underscore() {
        ThemeId::parse("brand-a_v2").expect("hyphen + underscore + digit must parse");
    }

    #[test]
    fn parse_rejects_empty() {
        assert_eq!(ThemeId::parse(""), Err(InvalidThemeId::Empty));
    }

    #[test]
    fn parse_rejects_too_short() {
        let err = ThemeId::parse("a").expect_err("single char must reject");
        assert!(matches!(err, InvalidThemeId::Length { len: 1, .. }));
    }

    #[test]
    fn parse_rejects_uppercase_lead() {
        let err = ThemeId::parse("Summus").expect_err("uppercase lead must reject");
        assert!(matches!(err, InvalidThemeId::Leading { found: 'S' }));
    }

    #[test]
    fn parse_rejects_digit_lead() {
        let err = ThemeId::parse("1summus").expect_err("digit lead must reject");
        assert!(matches!(err, InvalidThemeId::Leading { found: '1' }));
    }

    #[test]
    fn parse_rejects_dot() {
        let err = ThemeId::parse("a.b").expect_err("dot must reject");
        assert!(matches!(err, InvalidThemeId::Character { found: '.' }));
    }

    #[test]
    fn parse_rejects_space() {
        let err = ThemeId::parse("two words").expect_err("space must reject");
        assert!(matches!(err, InvalidThemeId::Character { found: ' ' }));
    }

    #[test]
    fn parse_rejects_overlong() {
        let long = "a".repeat(ThemeId::MAX_LEN + 1);
        let err = ThemeId::parse(&long).expect_err("over MAX_LEN must reject");
        assert!(matches!(err, InvalidThemeId::Length { .. }));
    }

    #[test]
    fn fromstr_matches_parse() {
        let parsed: ThemeId = "summus".parse().expect("FromStr must accept summus");
        assert_eq!(parsed, ThemeId::parse("summus").expect("parse must accept"));
    }

    #[test]
    fn display_round_trips() {
        let id = ThemeId::parse("summus").expect("parse must accept");
        assert_eq!(format!("{id}"), "summus");
    }

    #[test]
    fn deserialize_rejects_invalid() {
        let err: Result<ThemeId, _> = serde_json::from_str("\"BadName\"");
        assert!(err.is_err(), "uppercase must reject through serde path");
    }

    #[test]
    fn deserialize_accepts_valid() {
        let parsed: ThemeId =
            serde_json::from_str("\"summus\"").expect("valid id must round-trip via serde");
        assert_eq!(parsed.as_str(), "summus");
    }
}
