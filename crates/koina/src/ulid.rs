//! Internal ULID (Universally Unique Lexicographically Sortable Identifier)
//! implementation, replacing the external `ulid` crate.
//!
//! Spec: <https://github.com/ulid/spec>
//!
//! Layout: 128 bits = 48-bit millisecond timestamp + 80-bit random.
//! Encoding: 26-character Crockford base32, lexicographically sortable.

use std::fmt;
use std::fmt::Write as _;
use std::str::FromStr;
use std::time::SystemTime;

use rand::RngExt;
use serde::{Deserialize, Serialize};

fn decode_crockford(byte: u8) -> Option<u8> {
    match byte {
        b'0' | b'O' | b'o' => Some(0),
        b'1' | b'I' | b'i' | b'L' | b'l' => Some(1),
        b'2' => Some(2),
        b'3' => Some(3),
        b'4' => Some(4),
        b'5' => Some(5),
        b'6' => Some(6),
        b'7' => Some(7),
        b'8' => Some(8),
        b'9' => Some(9),
        b'A' | b'a' => Some(10),
        b'B' | b'b' => Some(11),
        b'C' | b'c' => Some(12),
        b'D' | b'd' => Some(13),
        b'E' | b'e' => Some(14),
        b'F' | b'f' => Some(15),
        b'G' | b'g' => Some(16),
        b'H' | b'h' => Some(17),
        b'J' | b'j' => Some(18),
        b'K' | b'k' => Some(19),
        b'M' | b'm' => Some(20),
        b'N' | b'n' => Some(21),
        b'P' | b'p' => Some(22),
        b'Q' | b'q' => Some(23),
        b'R' | b'r' => Some(24),
        b'S' | b's' => Some(25),
        b'T' | b't' => Some(26),
        b'V' | b'v' => Some(27),
        b'W' | b'w' => Some(28),
        b'X' | b'x' => Some(29),
        b'Y' | b'y' => Some(30),
        b'Z' | b'z' => Some(31),
        _ => None,
    }
}

fn encode_crockford(digit: u8) -> Option<char> {
    match digit {
        0 => Some('0'),
        1 => Some('1'),
        2 => Some('2'),
        3 => Some('3'),
        4 => Some('4'),
        5 => Some('5'),
        6 => Some('6'),
        7 => Some('7'),
        8 => Some('8'),
        9 => Some('9'),
        10 => Some('A'),
        11 => Some('B'),
        12 => Some('C'),
        13 => Some('D'),
        14 => Some('E'),
        15 => Some('F'),
        16 => Some('G'),
        17 => Some('H'),
        18 => Some('J'),
        19 => Some('K'),
        20 => Some('M'),
        21 => Some('N'),
        22 => Some('P'),
        23 => Some('Q'),
        24 => Some('R'),
        25 => Some('S'),
        26 => Some('T'),
        27 => Some('V'),
        28 => Some('W'),
        29 => Some('X'),
        30 => Some('Y'),
        31 => Some('Z'),
        _ => None,
    }
}

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
        let elapsed = match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
            Ok(duration) => duration,
            Err(_) => std::time::Duration::ZERO,
        };
        let ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);

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
        let [b0, b1, b2, b3, b4, b5, _, _, _, _, _, _, _, _, _, _] = self.0.to_be_bytes();
        u64::from_be_bytes([0, 0, b0, b1, b2, b3, b4, b5])
    }
}

impl Default for Ulid {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for Ulid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut val = self.0;
        let mut chars = ['0'; 26];
        for slot in chars.iter_mut().rev() {
            let Ok(digit) = u8::try_from(val & 0x1F) else {
                return Err(fmt::Error);
            };
            *slot = encode_crockford(digit).ok_or(fmt::Error)?;
            val >>= 5;
        }
        for ch in chars {
            f.write_char(ch)?;
        }
        Ok(())
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
            let digit = decode_crockford(byte).ok_or(DecodeError {
                reason: "invalid Crockford base32 character",
            })?;
            value = value
                .checked_shl(5)
                .ok_or(DecodeError { reason: "overflow" })?
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
        // First 4 chars decode to base32 values [0, 0, 1, 1].
        // Each char encodes 5 bits at decreasing shifts (125, 120, 115, 110);
        // the two zero chars contribute nothing, so only the two `1` chars remain.
        let expected_val = (1u128 << 115) | (1u128 << 110);
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
        let now_ms = u64::try_from(
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_millis(),
        )
        .unwrap();
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
