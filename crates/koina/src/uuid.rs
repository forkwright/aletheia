//! Clean-room UUID implementation covering v4 generation and v1 construction.
//!
//! WHY: The `uuid` crate is 8,574 LOC (~50-100KB) we don't need. UUID v4 is
//! just 16 random bytes with 6 bits fixed for version/variant. UUID v1 is a
//! timestamp + clock-seq + node packed per RFC 4122 §4.1. We use `rand` which
//! we already depend on for other purposes.
//!
//! This module now covers all UUID operations required by krites (formerly
//! delegated to the `uuid` crate): v4 generation, v1 construction, field
//! decomposition (`as_fields`/`from_fields`), byte-array access, nil check,
//! and v1 timestamp extraction. The `uuid` crate has been removed from the
//! workspace.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A UUID (128-bit identifier), supporting v4 and v1 variants.
///
/// WHY: Internal implementation eliminates the `uuid` crate dependency.
/// Supports all operations needed by the Datalog engine in krites:
/// v4 generation, v1 construction, RFC 4122 field decomposition, binary
/// serialization, and v1 timestamp extraction.
///
/// The underlying storage is always big-endian byte order per RFC 4122 §4.1.2.
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

    /// Create a UUID from a raw 128-bit value (big-endian byte order).
    ///
    /// WHY: Used for ULID-to-UUID conversion where the 128-bit value needs to
    /// be reinterpreted as a UUID for storage (#3101).
    #[must_use]
    pub fn from_u128(v: u128) -> Self {
        Self(v.to_be_bytes())
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
                    #[expect(
                        clippy::expect_used,
                        reason = "infallible: hex digit 0-15 always fits in u8"
                    )]
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

    /// Return the raw byte representation (big-endian, per RFC 4122 §4.1.2).
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    /// Construct a UUID from raw bytes (big-endian).
    #[must_use]
    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// Interpret the UUID as a big-endian u128.
    #[must_use]
    pub fn as_u128(&self) -> u128 {
        u128::from_be_bytes(self.0)
    }

    /// Return `true` if this is the nil UUID (all bytes zero).
    #[must_use]
    pub fn is_nil(&self) -> bool {
        self.0 == [0u8; 16]
    }

    /// Decompose into RFC 4122 fields: `(time_low, time_mid, time_hi_and_version, rest)`.
    ///
    /// The byte layout (RFC 4122 §4.1.2) is:
    /// - bytes 0–3  → `time_low` (`u32`, big-endian)
    /// - bytes 4–5  → `time_mid` (`u16`, big-endian)
    /// - bytes 6–7  → `time_hi_and_version` (`u16`, big-endian)
    /// - bytes 8–15 → variant + clock-seq + node (`[u8; 8]`)
    #[must_use]
    pub fn as_fields(&self) -> (u32, u16, u16, &[u8; 8]) {
        let time_low = u32::from_be_bytes([self.0[0], self.0[1], self.0[2], self.0[3]]);
        let time_mid = u16::from_be_bytes([self.0[4], self.0[5]]);
        let time_hi = u16::from_be_bytes([self.0[6], self.0[7]]);
        // Split the fixed-size array at a known constant boundary. This is safe
        // because `self.0` is `[u8; 16]` and `split_array_ref` is const-aware.
        let (_, rest_slice) = self.0.split_at(8);
        // INVARIANT: split_at(8) on [u8; 16] yields a 8-byte suffix.
        let rest: &[u8; 8] = rest_slice
            .try_into()
            .unwrap_or_else(|_| unreachable!("split_at(8) of [u8;16] always yields 8 bytes"));
        (time_low, time_mid, time_hi, rest)
    }

    /// Construct a UUID from RFC 4122 fields.
    ///
    /// Inverse of [`as_fields`](Self::as_fields). Bytes are written in big-endian
    /// order per RFC 4122 §4.1.2.
    #[must_use]
    pub fn from_fields(time_low: u32, time_mid: u16, time_hi: u16, rest: &[u8; 8]) -> Self {
        let mut bytes = [0u8; 16];
        bytes[0..4].copy_from_slice(&time_low.to_be_bytes());
        bytes[4..6].copy_from_slice(&time_mid.to_be_bytes());
        bytes[6..8].copy_from_slice(&time_hi.to_be_bytes());
        bytes[8..16].copy_from_slice(rest);
        Self(bytes)
    }

    /// Construct a UUID v1 from a 100-nanosecond timestamp, clock sequence, and node ID.
    ///
    /// Per RFC 4122 §4.2 the 60-bit timestamp is packed as:
    /// - bits 0–31  → `time_low` (bytes 0–3)
    /// - bits 32–47 → `time_mid` (bytes 4–5)
    /// - bits 48–59 → `time_high` (low 12 bits of bytes 6–7, high nibble = version 0x1)
    ///
    /// `timestamp_100ns` is the count of 100-nanosecond intervals since the UUID epoch
    /// (1582-10-15 00:00:00 UTC, i.e., `122_192_928_000_000_000` intervals before Unix epoch).
    #[must_use]
    #[expect(
        clippy::as_conversions,
        reason = "UUID field packing: masking before cast guarantees values fit in target types"
    )]
    pub fn new_v1(timestamp_100ns: u64, clock_seq: u16, node: &[u8; 6]) -> Self {
        // Masking ensures each field fits in the destination type before the `as` cast.
        // time_low: low 32 bits of 64-bit timestamp, masked to exactly 32 bits.
        let time_low = (timestamp_100ns & 0xFFFF_FFFF) as u32;
        // time_mid: bits 32–47, masked to 16 bits.
        let time_mid = ((timestamp_100ns >> 32) & 0xFFFF) as u16;
        // Set version = 1 in the high nibble of time_hi_and_version; bits 48–59.
        let time_hi = (((timestamp_100ns >> 48) & 0x0FFF) as u16) | 0x1000;
        // Set variant bits: RFC 4122 §4.1.1 — top two bits = 10.
        // clock_seq is u16; masking to 6 bits then casting to u8 is safe.
        let clock_seq_hi = (((clock_seq >> 8) & 0x3F) as u8) | 0x80;
        let clock_seq_low = (clock_seq & 0xFF) as u8;

        let mut bytes = [0u8; 16];
        bytes[0..4].copy_from_slice(&time_low.to_be_bytes());
        bytes[4..6].copy_from_slice(&time_mid.to_be_bytes());
        bytes[6..8].copy_from_slice(&time_hi.to_be_bytes());
        bytes[8] = clock_seq_hi;
        bytes[9] = clock_seq_low;
        bytes[10..16].copy_from_slice(node);
        Self(bytes)
    }

    /// Extract the v1 timestamp, returning `None` for non-v1 UUIDs.
    ///
    /// Returns a [`V1Timestamp`] whose [`to_unix`](V1Timestamp::to_unix) method
    /// yields `(unix_seconds: u64, subsec_100ns: u32)`.
    ///
    /// The 60-bit UUID v1 timestamp counts 100-nanosecond intervals from the UUID
    /// epoch (1582-10-15T00:00:00Z). The UUID epoch precedes the Unix epoch by
    /// exactly 122,192,928,000,000,000 intervals (≈ 122.19 billion seconds).
    ///
    /// RFC 4122 §4.1.4: the timestamp is stored split across three fields
    /// (`time_low`, `time_mid`, `time_high`) and must be reassembled by shifting.
    #[must_use]
    pub fn get_timestamp(&self) -> Option<V1Timestamp> {
        // Version is stored in the high nibble of byte 6
        let version = (self.0[6] >> 4) & 0xF;
        if version != 1 {
            return None;
        }
        let time_low = u64::from(u32::from_be_bytes([
            self.0[0], self.0[1], self.0[2], self.0[3],
        ]));
        let time_mid = u64::from(u16::from_be_bytes([self.0[4], self.0[5]]));
        let time_hi = u64::from(u16::from_be_bytes([self.0[6], self.0[7]]) & 0x0FFF);
        let ts_100ns = time_low | (time_mid << 32) | (time_hi << 48);
        Some(V1Timestamp(ts_100ns))
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

/// A v1 UUID timestamp extracted from a [`Uuid`].
///
/// Wraps the 60-bit 100-nanosecond counter measured from the UUID epoch
/// (1582-10-15T00:00:00Z, per RFC 4122 §4.1.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct V1Timestamp(u64);

impl V1Timestamp {
    /// Decompose into Unix seconds and sub-second 100-nanosecond units.
    ///
    /// The UUID epoch precedes the Unix epoch by exactly
    /// 122,192,928,000,000,000 × 100 ns intervals.
    ///
    /// Returns `(unix_seconds, subsec_100ns)` where `subsec_100ns` is in
    /// `[0, 10_000_000)` (there are 10,000,000 × 100 ns per second).
    #[must_use]
    pub fn to_unix(&self) -> (u64, u32) {
        // UUID epoch offset: 1582-10-15 → 1970-01-01 in 100-ns intervals.
        const UUID_UNIX_OFFSET: u64 = 122_192_928_000_000_000;
        // 10_000_000 × 100 ns = 1 second
        const INTERVALS_PER_SEC: u64 = 10_000_000;

        let ts = self.0.saturating_sub(UUID_UNIX_OFFSET);
        let secs = ts / INTERVALS_PER_SEC;
        // INVARIANT: remainder is always in 0..10_000_000 which fits u32.
        // `try_into()` encodes that fact without a silent `as` cast.
        let subsec = u32::try_from(ts % INTERVALS_PER_SEC)
            .unwrap_or_else(|_| unreachable!("remainder < 10_000_000 always fits u32"));
        (secs, subsec)
    }
}

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
    use proptest::prelude::*;

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
        let id = Uuid::default();
        let s = id.to_string();
        // Version should be 4
        assert_eq!(s.chars().nth(14), Some('4'));
    }

    #[test]
    fn as_bytes_from_bytes_roundtrip() {
        let id = Uuid::new_v4();
        let bytes = *id.as_bytes();
        let id2 = Uuid::from_bytes(bytes);
        assert_eq!(id, id2);
    }

    #[test]
    fn as_u128_from_u128_roundtrip() {
        let id = Uuid::new_v4();
        let n = id.as_u128();
        let id2 = Uuid::from_u128(n);
        assert_eq!(id, id2);
    }

    #[test]
    fn nil_uuid_is_nil() {
        let nil = Uuid::from_bytes([0u8; 16]);
        assert!(nil.is_nil());
        let non_nil = Uuid::new_v4();
        assert!(!non_nil.is_nil());
    }

    #[test]
    fn as_fields_from_fields_roundtrip() {
        let id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let (time_low, time_mid, time_hi, rest) = id.as_fields();
        let id2 = Uuid::from_fields(time_low, time_mid, time_hi, rest);
        assert_eq!(id, id2);
    }

    #[test]
    fn v1_uuid_timestamp_roundtrip() {
        // Known v1 UUID: "f3b4958c-52a1-11e7-802a-010203040506"
        // Generated 2017-06-16, Unix time ~1_497_624_119
        let v1 = Uuid::parse_str("f3b4958c-52a1-11e7-802a-010203040506").unwrap();
        let ts = v1.get_timestamp().expect("v1 uuid must have timestamp");
        let (secs, subsec) = ts.to_unix();
        // 2017-06-16 ≈ Unix time 1_497_600_000
        assert!(secs > 1_497_000_000, "seconds should be in 2017 range");
        assert!(subsec < 10_000_000, "subsec should be < 10M");
    }

    #[test]
    fn v4_uuid_has_no_timestamp() {
        let v4 = Uuid::new_v4();
        assert!(v4.get_timestamp().is_none());
    }

    #[test]
    fn new_v1_roundtrip_via_get_timestamp() {
        // Construct a v1 UUID from a known timestamp, verify extraction round-trips.
        // Timestamp: 2020-01-01T00:00:00Z in 100-ns since UUID epoch.
        const UUID_UNIX_OFFSET: u64 = 122_192_928_000_000_000;
        // 2020-01-01 00:00:00 UTC = 1577836800 seconds since Unix epoch
        let unix_secs: u64 = 1_577_836_800;
        let ts_100ns = unix_secs * 10_000_000 + UUID_UNIX_OFFSET;
        let node: [u8; 6] = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06];
        let id = Uuid::new_v1(ts_100ns, 0x0123, &node);

        // Version must be 1
        let s = id.to_string();
        assert_eq!(s.chars().nth(14), Some('1'), "version digit must be 1");

        let extracted = id.get_timestamp().expect("must extract v1 timestamp");
        let (secs, _subsec) = extracted.to_unix();
        assert_eq!(secs, unix_secs);
    }

    proptest::proptest! {
        #[test]
        fn prop_as_bytes_from_bytes_identity(bytes: [u8; 16]) {
            let id = Uuid::from_bytes(bytes);
            prop_assert_eq!(id.as_bytes(), &bytes);
        }

        #[test]
        fn prop_parse_str_to_string_roundtrip(bytes: [u8; 16]) {
            let id = Uuid::from_bytes(bytes);
            let s = id.to_string();
            let parsed = Uuid::parse_str(&s).unwrap();
            prop_assert_eq!(id, parsed);
        }

        #[test]
        fn prop_as_fields_from_fields_identity(bytes: [u8; 16]) {
            let id = Uuid::from_bytes(bytes);
            let (tl, tm, th, rest) = id.as_fields();
            let id2 = Uuid::from_fields(tl, tm, th, rest);
            prop_assert_eq!(id, id2);
        }
    }
}
