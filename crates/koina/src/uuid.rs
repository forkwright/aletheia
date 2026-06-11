//! Clean-room UUID implementation covering v4 generation and v1 construction.
//!
//! WHY: keep UUID handling local instead of depending on the external `uuid`
//! crate; this module covers the v4/v1 operations krites actually needs.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A UUID (128-bit identifier), supporting v4 and v1 variants.
///
/// WHY: this internal type covers the v4/v1 operations krites needs.
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
        if let Some(version) = bytes.get_mut(6) {
            *version = (*version & 0x0F) | 0x40;
        }
        if let Some(variant) = bytes.get_mut(8) {
            *variant = (*variant & 0x3F) | 0x80;
        }
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
                    let Ok(nibble) = u8::try_from(c.to_digit(16).ok_or(UuidParseError)?) else {
                        return Err(UuidParseError);
                    };
                    let byte = bytes.get_mut(byte_idx / 2).ok_or(UuidParseError)?;
                    if byte_idx % 2 == 0 {
                        *byte = nibble << 4;
                    } else {
                        *byte |= nibble;
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
        static ZERO_REST: [u8; 8] = [0; 8];
        let (head, rest) = self.0.split_at(8);
        let Ok(head) = <&[u8; 8]>::try_from(head) else {
            return (0, 0, 0, &ZERO_REST);
        };
        let Ok(rest) = <&[u8; 8]>::try_from(rest) else {
            return (0, 0, 0, &ZERO_REST);
        };
        let [b0, b1, b2, b3, b4, b5, b6, b7] = *head;
        let time_low = u32::from_be_bytes([b0, b1, b2, b3]);
        let time_mid = u16::from_be_bytes([b4, b5]);
        let time_hi = u16::from_be_bytes([b6, b7]);
        (time_low, time_mid, time_hi, rest)
    }

    /// Construct a UUID from RFC 4122 fields.
    ///
    /// Inverse of [`as_fields`](Self::as_fields). Bytes are written in big-endian
    /// order per RFC 4122 §4.1.2.
    #[must_use]
    pub fn from_fields(time_low: u32, time_mid: u16, time_hi: u16, rest: &[u8; 8]) -> Self {
        let mut bytes = [0u8; 16];
        let [
            b0,
            b1,
            b2,
            b3,
            b4,
            b5,
            b6,
            b7,
            b8,
            b9,
            b10,
            b11,
            b12,
            b13,
            b14,
            b15,
        ] = &mut bytes;
        let [tl0, tl1, tl2, tl3] = time_low.to_be_bytes();
        let [tm0, tm1] = time_mid.to_be_bytes();
        let [th0, th1] = time_hi.to_be_bytes();
        let [r0, r1, r2, r3, r4, r5, r6, r7] = *rest;
        *b0 = tl0;
        *b1 = tl1;
        *b2 = tl2;
        *b3 = tl3;
        *b4 = tm0;
        *b5 = tm1;
        *b6 = th0;
        *b7 = th1;
        *b8 = r0;
        *b9 = r1;
        *b10 = r2;
        *b11 = r3;
        *b12 = r4;
        *b13 = r5;
        *b14 = r6;
        *b15 = r7;
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
    pub fn new_v1(timestamp_100ns: u64, clock_seq: u16, node: &[u8; 6]) -> Self {
        let [_, _, _, _, ts4, ts5, ts6, ts7] = timestamp_100ns.to_be_bytes();
        let time_low = u32::from_be_bytes([ts4, ts5, ts6, ts7]);
        let time_mid = u16::from_be_bytes([ts2(timestamp_100ns), ts3(timestamp_100ns)]);
        let time_hi =
            u16::from_be_bytes([ts0(timestamp_100ns) & 0x0F, ts1(timestamp_100ns)]) | 0x1000;
        let [seq_hi, seq_low] = clock_seq.to_be_bytes();
        let clock_seq_hi = (seq_hi & 0x3F) | 0x80;
        let clock_seq_low = seq_low;

        let mut bytes = [0u8; 16];
        let [
            b0,
            b1,
            b2,
            b3,
            b4,
            b5,
            b6,
            b7,
            b8,
            b9,
            b10,
            b11,
            b12,
            b13,
            b14,
            b15,
        ] = &mut bytes;
        let [tl0, tl1, tl2, tl3] = time_low.to_be_bytes();
        let [tm0, tm1] = time_mid.to_be_bytes();
        let [th0, th1] = time_hi.to_be_bytes();
        let [n0, n1, n2, n3, n4, n5] = *node;
        *b0 = tl0;
        *b1 = tl1;
        *b2 = tl2;
        *b3 = tl3;
        *b4 = tm0;
        *b5 = tm1;
        *b6 = th0;
        *b7 = th1;
        *b8 = clock_seq_hi;
        *b9 = clock_seq_low;
        *b10 = n0;
        *b11 = n1;
        *b12 = n2;
        *b13 = n3;
        *b14 = n4;
        *b15 = n5;
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
        let [b0, b1, b2, b3, b4, b5, b6, b7, _, _, _, _, _, _, _, _] = self.0;
        // Version is stored in the high nibble of byte 6.
        let version = (b6 >> 4) & 0xF;
        if version != 1 {
            return None;
        }
        let time_low = u64::from(u32::from_be_bytes([b0, b1, b2, b3]));
        let time_mid = u64::from(u16::from_be_bytes([b4, b5]));
        let time_hi = u64::from(u16::from_be_bytes([b6, b7]) & 0x0FFF);
        let ts_100ns = time_low | (time_mid << 32) | (time_hi << 48);
        Some(V1Timestamp(ts_100ns))
    }

    /// Format as hyphenated string.
    fn hyphenated(&self) -> impl fmt::Display + '_ {
        struct Hyphenated<'a>(&'a [u8; 16]);
        impl fmt::Display for Hyphenated<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                let [
                    b0,
                    b1,
                    b2,
                    b3,
                    b4,
                    b5,
                    b6,
                    b7,
                    b8,
                    b9,
                    b10,
                    b11,
                    b12,
                    b13,
                    b14,
                    b15,
                ] = *self.0;
                write!(
                    f,
                    "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
                    u32::from_be_bytes([b0, b1, b2, b3]),
                    u16::from_be_bytes([b4, b5]),
                    u16::from_be_bytes([b6, b7]),
                    u16::from_be_bytes([b8, b9]),
                    u64::from_be_bytes([0, 0, b10, b11, b12, b13, b14, b15])
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
        let Ok(subsec) = u32::try_from(ts % INTERVALS_PER_SEC) else {
            return (secs, 0);
        };
        (secs, subsec)
    }
}

fn ts0(timestamp_100ns: u64) -> u8 {
    let [b0, _, _, _, _, _, _, _] = timestamp_100ns.to_be_bytes();
    b0
}

fn ts1(timestamp_100ns: u64) -> u8 {
    let [_, b1, _, _, _, _, _, _] = timestamp_100ns.to_be_bytes();
    b1
}

fn ts2(timestamp_100ns: u64) -> u8 {
    let [_, _, b2, _, _, _, _, _] = timestamp_100ns.to_be_bytes();
    b2
}

fn ts3(timestamp_100ns: u64) -> u8 {
    let [_, _, _, b3, _, _, _, _] = timestamp_100ns.to_be_bytes();
    b3
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
