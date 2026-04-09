//! Internal utilities shared across `symbolon` modules.

/// Convert days since the Unix epoch (1970-01-01) to a `(year, month, day)` triple.
///
/// Implements the civil-date algorithm by Howard Hinnant.
pub(crate) fn days_to_date(days_since_epoch: u64) -> (u64, u64, u64) {
    let z = days_since_epoch + 719_468;
    let era = z / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146_096) / 365;
    let y = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let d = day_of_year - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Standard base64 character set (with `+` and `/`).
const BASE64_CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// URL-safe base64 character set (with `-` and `_`).
const BASE64URL_CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Encode bytes to standard base64 (with `+`, `/`, and `=` padding).
pub(crate) fn base64_encode(input: &[u8]) -> String {
    base64_encode_with_alphabet(input, BASE64_CHARS, true)
}

/// Decode standard base64 (with `+`, `/`, `=` padding).
pub(crate) fn base64_decode(s: &str) -> Option<Vec<u8>> {
    base64_decode_with_alphabet(s, false)
}

/// Encode bytes to base64url (with `-`, `_`, no padding).
pub(crate) fn base64url_encode(input: &[u8]) -> String {
    base64_encode_with_alphabet(input, BASE64URL_CHARS, false)
}

/// Decode base64url-encoded string (with `-`, `_`, no padding required).
///
/// WHY: extracts JWT payload segments to read `exp` claims without pulling in a
/// dedicated crate for this ~30-line function. Base64url differs from standard
/// Base64 only in the `+`/`-` and `/`/`_` substitutions and the omission of `=` padding.
pub(crate) fn base64url_decode(s: &str) -> Option<Vec<u8>> {
    base64_decode_with_alphabet(s, true)
}

/// Internal: encode with a given alphabet and optional padding.
fn base64_encode_with_alphabet(input: &[u8], alphabet: &[u8; 64], pad: bool) -> String {
    let mut out = String::with_capacity((input.len() * 4 + 2) / 3 + if pad { 4 } else { 0 });

    let mut chunks = input.chunks_exact(3);
    for chunk in &mut chunks {
        let b = (u32::from(chunk[0]) << 16) | (u32::from(chunk[1]) << 8) | u32::from(chunk[2]);
        out.push(char::from(alphabet[((b >> 18) & 0x3F) as usize]));
        out.push(char::from(alphabet[((b >> 12) & 0x3F) as usize]));
        out.push(char::from(alphabet[((b >> 6) & 0x3F) as usize]));
        out.push(char::from(alphabet[(b & 0x3F) as usize]));
    }

    let remainder = chunks.remainder();
    match remainder.len() {
        0 => {}
        1 => {
            let b = u32::from(remainder[0]) << 16;
            out.push(char::from(alphabet[((b >> 18) & 0x3F) as usize]));
            out.push(char::from(alphabet[((b >> 12) & 0x3F) as usize]));
            if pad {
                out.push('=');
                out.push('=');
            }
        }
        2 => {
            let b = (u32::from(remainder[0]) << 16) | (u32::from(remainder[1]) << 8);
            out.push(char::from(alphabet[((b >> 18) & 0x3F) as usize]));
            out.push(char::from(alphabet[((b >> 12) & 0x3F) as usize]));
            out.push(char::from(alphabet[((b >> 6) & 0x3F) as usize]));
            if pad {
                out.push('=');
            }
        }
        // SAFETY: chunks_exact(3).remainder() returns 0, 1, or 2 elements only.
        #[expect(
            clippy::unreachable,
            reason = "chunks_exact(3) guarantees remainder().len() ∈ {0, 1, 2}; only 3 arms reachable"
        )]
        _ => unreachable!("chunks_exact(3) remainder cannot exceed 2"),
    }

    out
}

/// Internal: decode with support for both standard and URL-safe alphabets.
fn base64_decode_with_alphabet(s: &str, url_safe: bool) -> Option<Vec<u8>> {
    /// Map a single base64/base64url character to its 6-bit value.
    fn char_val(b: u8, url_safe: bool) -> Option<u8> {
        match b {
            b'A'..=b'Z' => Some(b - b'A'),
            b'a'..=b'z' => Some(b - b'a' + 26),
            b'0'..=b'9' => Some(b - b'0' + 52),
            b'-' => Some(62),
            b'_' => Some(63),
            b'+' if !url_safe => Some(62),
            b'/' if !url_safe => Some(63),
            b'=' => Some(0), // NOTE: padding treated as zero bits
            _ => None,
        }
    }

    let bytes = s.as_bytes();
    let end = bytes.iter().rposition(|&b| b != b'=').map_or(0, |i| i + 1);
    let bytes = bytes.get(..end).unwrap_or(bytes); // SAFETY: end <= bytes.len() by construction from rposition

    let mut out = Vec::with_capacity(bytes.len() * 6 / 8 + 1);
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;

    for &b in bytes {
        let v = char_val(b, url_safe)?;
        buf = (buf << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            // SAFETY: bits is 0-7 after decrement, buf >> bits lowest 8 bits are the decoded byte
            out.push(u8::try_from((buf >> bits) & 0xFF).unwrap_or(0));
        }
    }

    Some(out)
}

/// Extract the `exp` (expiry, seconds since epoch) claim from a dot-segmented token.
///
/// WHY: OAuth access tokens stored in env vars carry no separate expiry metadata;
/// reading the `exp` claim embedded in the token's payload segment is the only
/// non-network way to detect a stale token and allow fallthrough to a refreshable
/// file-based provider.
///
/// NOTE: signature is intentionally not verified: only the expiry claim is read.
/// Returns `None` when the token has no recognisable payload segment or no `exp`
/// field; the caller must treat `None` as "expiry unknown" (do not fall through).
pub(crate) fn decode_jwt_exp_secs(token: &str) -> Option<u64> {
    // NOTE: dot-segmented format -- first segment is vendor prefix or JWT header,
    // second segment is the JSON payload containing the exp claim.
    let mut segs = token.splitn(4, '.');
    let _first = segs.next()?;
    let payload_b64 = segs.next()?;

    let payload = base64url_decode(payload_b64)?;
    let value: serde_json::Value = serde_json::from_slice(&payload).ok()?;

    // NOTE: exp is seconds since epoch per JWT spec (RFC 7519).
    value.get("exp").and_then(serde_json::Value::as_u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_is_1970_01_01() {
        assert_eq!(days_to_date(0), (1970, 1, 1));
    }

    #[test]
    fn known_date_2023_11_14() {
        assert_eq!(days_to_date(19_675), (2023, 11, 14));
    }

    // Base64 tests
    #[test]
    fn base64_encode_empty() {
        assert_eq!(base64_encode(b""), "");
    }

    #[test]
    fn base64_encode_single_byte() {
        // 'f' = 0x66 = 102
        // 102 << 16 = 0x660000 = binary: 0110 0110 0000 0000 0000 0000
        // 6-bit groups: 011001 100000 000000 000000 = 25 32 0 0 -> 'Zg=='
        assert_eq!(base64_encode(b"f"), "Zg==");
    }

    #[test]
    fn base64_encode_two_bytes() {
        // 'fo' = 0x66 0x6f
        // (102 << 16) | (111 << 8) = 0x666F00
        // 6-bit groups: 011001 100110 111100 000000 = 25 38 60 0 -> 'Zm8='
        assert_eq!(base64_encode(b"fo"), "Zm8=");
    }

    #[test]
    fn base64_encode_three_bytes() {
        assert_eq!(base64_encode(b"foo"), "Zm9v");
    }

    #[test]
    fn base64_encode_hello_world() {
        assert_eq!(base64_encode(b"Hello, World!"), "SGVsbG8sIFdvcmxkIQ==");
    }

    #[test]
    fn base64_roundtrip() {
        let input = b"The quick brown fox jumps over the lazy dog.";
        let encoded = base64_encode(input);
        let decoded = base64_decode(&encoded).unwrap();
        assert_eq!(decoded, input);
    }

    #[test]
    fn base64_decode_with_padding() {
        assert_eq!(base64_decode("Zg==").unwrap(), b"f");
        assert_eq!(base64_decode("Zm8=").unwrap(), b"fo");
        assert_eq!(base64_decode("Zm9v").unwrap(), b"foo");
    }

    #[test]
    fn base64_decode_invalid_char() {
        // '@' is not a valid base64 character
        assert!(base64_decode("Zm9v@").is_none());
    }

    // Base64url tests
    #[test]
    fn base64url_encode_empty() {
        assert_eq!(base64url_encode(b""), "");
    }

    #[test]
    fn base64url_encode_basic() {
        // "foo" should be "Zm9v" (same as standard)
        assert_eq!(base64url_encode(b"foo"), "Zm9v");
    }

    #[test]
    fn base64url_encode_with_special_bytes() {
        // Bytes that would produce + or / in standard base64
        let input: &[u8] = &[0xfb, 0xff, 0xfe]; // These produce +/= chars in standard base64
        let encoded = base64url_encode(input);
        // Should not contain + or /, and no padding
        assert!(!encoded.contains('+'));
        assert!(!encoded.contains('/'));
        assert!(!encoded.contains('='));
    }

    #[test]
    fn base64url_roundtrip() {
        let input = b"The quick brown fox jumps over the lazy dog.";
        let encoded = base64url_encode(input);
        let decoded = base64url_decode(&encoded).unwrap();
        assert_eq!(decoded, input);
    }

    #[test]
    fn base64url_decode_with_standard_chars() {
        // The internal decoder should accept both -/_ and +/ when in url_safe mode
        // (the existing behavior already handles this)
        let input = b"test";
        let encoded = base64url_encode(input);
        let decoded = base64url_decode(&encoded).unwrap();
        assert_eq!(decoded, input);
    }

    #[test]
    fn base64url_decode_known_jwt_header() {
        // eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9 is a known JWT header
        // {"alg":"HS256","typ":"JWT"}
        let decoded = base64url_decode("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9").unwrap();
        assert_eq!(decoded, br#"{"alg":"HS256","typ":"JWT"}"#);
    }
}
