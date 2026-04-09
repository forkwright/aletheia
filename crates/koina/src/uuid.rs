//! Internal UUID v4 generation (dependency-free).
//!
//! WHY: The `uuid` crate is ~50-100KB of code we don't need. UUID v4 is just
//! 16 random bytes with 6 bits fixed for version/variant. We use `rand` which
//! we already depend on for other purposes.
//!
//! For krites (CozoDB), we keep the `uuid` crate due to complex binary format
//! requirements (v1 timestamps, from_fields/as_fields, etc.).

use std::fmt;

use serde::{Deserialize, Serialize};
/// A UUID v4 (128-bit random identifier).
///
/// WHY: Internal implementation eliminates the `uuid` crate dependency for
/// simple v4 generation use cases. This is NOT a general-purpose UUID library;
/// it only supports what Aletheia needs: v4 generation and hyphenated formatting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Uuid([u8; 16]);

impl Uuid {
    /// Generate a new random UUID v4.
    ///
    /// Uses `rand::random()` for the underlying randomness.
    #[must_use]
    pub fn new_v4() -> Self {
        let mut bytes: [u8; 16] = rand::random();
        // Set version (4) in the high nibble of byte 6
        bytes[6] = (bytes[6] & 0x0F) | 0x40;
        // Set variant (RFC 4122) in the high two bits of byte 8
        bytes[8] = (bytes[8] & 0x3F) | 0x80;
        Self(bytes)
    }

    /// Parse a UUID from a hyphenated string (e.g., "550e8400-e29b-41d4-a716-446655440000").
    ///
    /// # Errors
    /// Returns an error if the string is not a valid UUID format.
    pub fn parse_str(s: &str) -> Result<Self, UuidParseError> {
        // Expected format: xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx (36 chars)
        if s.len() != 36 {
            return Err(UuidParseError);
        }

        let mut bytes = [0u8; 16];
        let mut byte_idx = 0;

        for (i, c) in s.chars().enumerate() {
            match i {
                8 | 13 | 18 | 23 => {
                    if c != '-' {
                        return Err(UuidParseError);
                    }
                }
                _ => {
                    // SAFETY: `to_digit(16)` returns 0-15, which always fits in u8.
                    // This is an infallible conversion, but `try_into()` is used
                    // for type safety and to satisfy clippy::as_conversions.
                    #[expect(clippy::expect_used, reason = "infallible: hex digit 0-15 always fits in u8")]
                    let nibble: u8 = c
                        .to_digit(16)
                        .ok_or(UuidParseError)?
                        .try_into()
                        .expect("hex digit 0-15 always fits in u8");
                    // WHY: `byte_idx` only increments inside this branch, and
                    // this branch runs for the 32 hex chars in a 36-char UUID
                    // (positions 0-7, 9-12, 14-17, 19-22, 24-35). So byte_idx
                    // ranges 0..32 and `byte_idx / 2` ranges 0..16 — exactly
                    // the bounds of `bytes: [u8; 16]`. Safe by construction.
                    #[expect(
                        clippy::indexing_slicing,
                        reason = "byte_idx in 0..32; byte_idx/2 in 0..16; bytes is [u8; 16]"
                    )]
                    if byte_idx % 2 == 0 {
                        bytes[byte_idx / 2] = nibble << 4;
                    } else {
                        bytes[byte_idx / 2] |= nibble;
                    }
                    byte_idx += 1;
                }
            }
        }

        Ok(Self(bytes))
    }

    /// Format as hyphenated string.
    fn hyphenated(&self) -> impl fmt::Display + '_ {
        struct Hyphenated<'a>(&'a [u8; 16]);
        impl fmt::Display for Hyphenated<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(
                    f,
                    "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
                    u32::from_be_bytes([self.0[0], self.0[1], self.0[2], self.0[3]]),
                    u16::from_be_bytes([self.0[4], self.0[5]]),
                    u16::from_be_bytes([self.0[6], self.0[7]]),
                    u16::from_be_bytes([self.0[8], self.0[9]]),
                    u64::from_be_bytes([
                        0, 0, self.0[10], self.0[11], self.0[12], self.0[13], self.0[14],
                        self.0[15],
                    ])
                )
            }
        }
        Hyphenated(&self.0)
    }
}

impl fmt::Display for Uuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.hyphenated())
    }
}

impl Default for Uuid {
    fn default() -> Self {
        Self::new_v4()
    }
}

/// Error when parsing a UUID string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UuidParseError;

impl fmt::Display for UuidParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid UUID format")
    }
}

impl std::error::Error for UuidParseError {}

/// Generate a random UUID v4 as a string.
///
/// Convenience function for the common case of needing a UUID string
/// without the overhead of the `Uuid` type.
#[must_use]
pub fn uuid_v4() -> String {
    Uuid::new_v4().to_string()
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn uuid_v4_format_is_valid() {
        let id = Uuid::new_v4();
        let s = id.to_string();

        // Should be 36 characters: 8-4-4-4-12
        assert_eq!(s.len(), 36);

        // Check hyphens are in right positions
        assert_eq!(s.chars().nth(8), Some('-'));
        assert_eq!(s.chars().nth(13), Some('-'));
        assert_eq!(s.chars().nth(18), Some('-'));
        assert_eq!(s.chars().nth(23), Some('-'));

        // Version should be 4 (first nibble of 3rd segment)
        assert_eq!(s.chars().nth(14), Some('4'));

        // Variant should be 8, 9, a, or b (first nibble of 4th segment)
        let variant_char = s.chars().nth(19).unwrap();
        assert!(
            variant_char == '8'
                || variant_char == '9'
                || variant_char == 'a'
                || variant_char == 'b',
            "variant should be RFC 4122 (8,9,a,b), got {variant_char}"
        );
    }

    #[test]
    fn uuid_v4_unique() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        assert_ne!(a, b);
    }

    #[test]
    fn uuid_parse_roundtrip() {
        let original = Uuid::new_v4();
        let s = original.to_string();
        let parsed = Uuid::parse_str(&s).unwrap();
        assert_eq!(original, parsed);
    }

    #[test]
    fn uuid_parse_valid() {
        let valid = "550e8400-e29b-41d4-a716-446655440000";
        let parsed = Uuid::parse_str(valid).unwrap();
        assert_eq!(parsed.to_string(), valid);
    }

    #[test]
    fn uuid_parse_invalid_formats() {
        assert!(Uuid::parse_str("").is_err());
        assert!(Uuid::parse_str("not-a-uuid").is_err());
        assert!(Uuid::parse_str("550e8400e29b41d4a716446655440000").is_err()); // no hyphens
        assert!(Uuid::parse_str("550e8400-e29b-41d4-a716-44665544000").is_err()); // too short
        assert!(Uuid::parse_str("550e8400-e29b-41d4-a716-4466554400000").is_err()); // too long
        assert!(Uuid::parse_str("550e8400_e29b_41d4_a716_446655440000").is_err()); // underscores
    }

    #[test]
    fn uuid_v4_helper_returns_string() {
        let s = uuid_v4();
        assert_eq!(s.len(), 36);
        assert!(s.contains('-'));
    }

    #[test]
    fn default_is_v4() {
        let id: Uuid = Default::default();
        let s = id.to_string();
        // Version should be 4
        assert_eq!(s.chars().nth(14), Some('4'));
    }
}
