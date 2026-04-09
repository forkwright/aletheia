//! Internal ULID (Universally Unique Lexicographically Sortable Identifier)
//! implementation, replacing the external `ulid` crate.
//!
//! Spec: <https://github.com/ulid/spec>
//!
//! Layout: 128 bits = 48-bit millisecond timestamp + 80-bit random.
//! Encoding: 26-character Crockford base32, lexicographically sortable.

use std::fmt;
use std::str::FromStr;
use std::time::SystemTime;

use rand::Rng;
use serde::{Deserialize, Serialize};

/// Crockford base32 alphabet (uppercase).
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Reverse lookup: ASCII byte → 5-bit value. 0xFF = invalid.
const DECODE: [u8; 128] = {
    let mut table = [0xFFu8; 128];
    // Digits
    table[b'0' as usize] = 0;
    table[b'1' as usize] = 1;
    table[b'2' as usize] = 2;
    table[b'3' as usize] = 3;
    table[b'4' as usize] = 4;
    table[b'5' as usize] = 5;
    table[b'6' as usize] = 6;
    table[b'7' as usize] = 7;
    table[b'8' as usize] = 8;
    table[b'9' as usize] = 9;
    // Upper
    table[b'A' as usize] = 10;
    table[b'B' as usize] = 11;
    table[b'C' as usize] = 12;
    table[b'D' as usize] = 13;
    table[b'E' as usize] = 14;
    table[b'F' as usize] = 15;
    table[b'G' as usize] = 16;
    table[b'H' as usize] = 17;
    table[b'J' as usize] = 18;
    table[b'K' as usize] = 19;
    table[b'M' as usize] = 20;
    table[b'N' as usize] = 21;
    table[b'P' as usize] = 22;
    table[b'Q' as usize] = 23;
    table[b'R' as usize] = 24;
    table[b'S' as usize] = 25;
    table[b'T' as usize] = 26;
    table[b'V' as usize] = 27;
    table[b'W' as usize] = 28;
    table[b'X' as usize] = 29;
    table[b'Y' as usize] = 30;
    table[b'Z' as usize] = 31;
    // Lower (same values)
    table[b'a' as usize] = 10;
    table[b'b' as usize] = 11;
    table[b'c' as usize] = 12;
    table[b'd' as usize] = 13;
    table[b'e' as usize] = 14;
    table[b'f' as usize] = 15;
    table[b'g' as usize] = 16;
    table[b'h' as usize] = 17;
    table[b'j' as usize] = 18;
    table[b'k' as usize] = 19;
    table[b'm' as usize] = 20;
    table[b'n' as usize] = 21;
    table[b'p' as usize] = 22;
    table[b'q' as usize] = 23;
    table[b'r' as usize] = 24;
    table[b's' as usize] = 25;
    table[b't' as usize] = 26;
    table[b'v' as usize] = 27;
    table[b'w' as usize] = 28;
    table[b'x' as usize] = 29;
    table[b'y' as usize] = 30;
    table[b'z' as usize] = 31;
    // Confusable aliases per Crockford spec
    table[b'O' as usize] = 0; // O → 0
    table[b'o' as usize] = 0;
    table[b'I' as usize] = 1; // I → 1
    table[b'i' as usize] = 1;
    table[b'L' as usize] = 1; // L → 1
    table[b'l' as usize] = 1;
    table
};

/// A ULID (Universally Unique Lexicographically Sortable Identifier).
///
/// 128 bits: 48-bit millisecond timestamp (upper) + 80-bit random (lower).
/// Encodes as 26 Crockford base32 characters.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ulid(u128);

impl Ulid {
    /// Generate a new ULID using the current system time and random entropy.
    #[must_use]
    pub fn new() -> Self {
        // WHY: Duration::as_millis returns u128, but the ULID spec uses a
        // 48-bit ms timestamp (the upper 16 bits of the u64 are unused).
        // try_from with saturating fallback to u64::MAX is correct: any
        // value above 2^48 (year 8921) is already a spec violation; the
        // saturating cast is documented and avoids the silent `as`.
        let ms = u64::try_from(
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
        )
        .unwrap_or(u64::MAX);

        let random: u128 = {
            let mut rng = rand::rng();
            // WHY: 80 bits of randomness from two random values masked to fit
            let hi: u64 = rng.random();
            let lo: u16 = rng.random();
            (u128::from(hi) << 16) | u128::from(lo)
        };

        // Upper 48 bits: timestamp. Lower 80 bits: random.
        let value = (u128::from(ms) << 80) | (random & ((1u128 << 80) - 1));
        Self(value)
    }

    /// Create a ULID from a raw 128-bit value.
    #[must_use]
    pub const fn from_u128(value: u128) -> Self {
        Self(value)
    }

    /// The raw 128-bit value.
    #[must_use]
    pub const fn as_u128(self) -> u128 {
        self.0
    }

    /// The millisecond timestamp component (upper 48 bits).
    #[must_use]
    pub const fn timestamp_ms(self) -> u64 {
        (self.0 >> 80) as u64
    }
}

impl Default for Ulid {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for Ulid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut buf = [0u8; 26];
        let mut val = self.0;
        // Encode from least significant to most significant
        for byte in buf.iter_mut().rev() {
            *byte = CROCKFORD[(val & 0x1F) as usize];
            val >>= 5;
        }
        // WHY: CROCKFORD alphabet is ASCII (32 bytes), so every byte we
        // emit into `buf` is in 0..=127 — valid UTF-8 by construction.
        // Use #[expect] on the local expect() so the invariant is documented
        // at the call site rather than via unsafe.
        #[expect(
            clippy::expect_used,
            reason = "Crockford base32 alphabet is ASCII; buf is always valid UTF-8 by construction"
        )]
        let s = std::str::from_utf8(&buf).expect("Crockford base32 is always valid UTF-8");
        f.write_str(s)
    }
}

impl fmt::Debug for Ulid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Ulid({self})")
    }
}

/// Error returned when parsing an invalid ULID string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeError {
    reason: &'static str,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid ULID: {}", self.reason)
    }
}

impl std::error::Error for DecodeError {}

impl FromStr for Ulid {
    type Err = DecodeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 26 {
            return Err(DecodeError {
                reason: "must be exactly 26 characters",
            });
        }

        let mut value: u128 = 0;
        for &byte in s.as_bytes() {
            if byte >= 128 {
                return Err(DecodeError {
                    reason: "non-ASCII character",
                });
            }
            let digit = DECODE[byte as usize];
            if digit == 0xFF {
                return Err(DecodeError {
                    reason: "invalid Crockford base32 character",
                });
            }
            value = value
                .checked_shl(5)
                .ok_or(DecodeError {
                    reason: "overflow",
                })?
                | u128::from(digit);
        }

        Ok(Self(value))
    }
}

impl Serialize for Ulid {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Ulid {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn new_produces_26_char_string() {
        let ulid = Ulid::new();
        let s = ulid.to_string();
        assert_eq!(s.len(), 26, "ULID string must be 26 characters");
        assert!(
            s.chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()),
            "ULID must be uppercase Crockford base32"
        );
    }

    #[test]
    fn roundtrip_display_parse() {
        let ulid = Ulid::new();
        let s = ulid.to_string();
        let back: Ulid = s.parse().unwrap();
        assert_eq!(ulid, back);
    }

    #[test]
    fn lowercase_parse() {
        let ulid = Ulid::new();
        let lower = ulid.to_string().to_lowercase();
        let back: Ulid = lower.parse().unwrap();
        assert_eq!(ulid, back);
    }

    #[test]
    fn confusable_aliases() {
        // O → 0, I/L → 1 per Crockford spec
        let s = "0OIL0000000000000000000000";
        assert_eq!(s.len(), 26);
        let ulid: Ulid = s.parse().unwrap();
        // First 4 chars decode as: 0, 0, 1, 1
        let expected_val = (0u128 << 125) | (0u128 << 120) | (1u128 << 115) | (1u128 << 110);
        assert_eq!(ulid.as_u128(), expected_val);
    }

    #[test]
    fn invalid_length_rejected() {
        assert!("short".parse::<Ulid>().is_err());
        assert!("000000000000000000000000000".parse::<Ulid>().is_err()); // 27 chars
    }

    #[test]
    fn invalid_char_rejected() {
        assert!("0000000000000000000000000U".parse::<Ulid>().is_err()); // U not in Crockford
    }

    #[test]
    fn timestamp_extraction() {
        let ulid = Ulid::new();
        let ts = ulid.timestamp_ms();
        let now_ms = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        // Should be within 1 second of now
        assert!(
            ts.abs_diff(now_ms) < 1000,
            "timestamp {ts} should be close to {now_ms}"
        );
    }

    #[test]
    fn lexicographic_ordering() {
        // ULIDs generated later should sort after earlier ones
        let a = Ulid::new();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let b = Ulid::new();
        assert!(a.to_string() < b.to_string(), "later ULID must sort after");
        assert!(a < b, "Ord must match timestamp ordering");
    }

    #[test]
    fn uniqueness() {
        let a = Ulid::new();
        let b = Ulid::new();
        assert_ne!(a, b, "two ULIDs must differ (80 bits of randomness)");
    }

    #[test]
    fn serde_roundtrip() {
        let ulid = Ulid::new();
        let json = serde_json::to_string(&ulid).unwrap();
        let back: Ulid = serde_json::from_str(&json).unwrap();
        assert_eq!(ulid, back);
        // Serialized as quoted string
        assert!(json.starts_with('"') && json.ends_with('"'));
    }

    #[test]
    fn zero_value() {
        let zero = Ulid::from_u128(0);
        assert_eq!(zero.to_string(), "00000000000000000000000000");
        assert_eq!(zero.timestamp_ms(), 0);
    }

    #[test]
    fn max_value() {
        let max = Ulid::from_u128(u128::MAX);
        let s = max.to_string();
        assert_eq!(s, "7ZZZZZZZZZZZZZZZZZZZZZZZZZ");
    }

    #[test]
    fn debug_format() {
        let ulid = Ulid::from_u128(0);
        let debug = format!("{ulid:?}");
        assert!(debug.starts_with("Ulid("));
    }
}
