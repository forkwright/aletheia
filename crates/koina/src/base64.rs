//! RFC 4648 base64 encoding and decoding.
//!
//! Provides standard (padded) and URL-safe (no-pad) variants.
//! No SIMD, no streaming — straightforward iterator-based implementation.

use snafu::Snafu;

/// Errors that can occur during base64 decoding.
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub enum DecodeError {
    /// Input contained a character not in the base64 alphabet.
    #[snafu(display("invalid base64 character: {ch} at position {position}"))]
    InvalidChar {
        /// The offending character.
        ch: char,
        /// Byte position in the input string.
        position: usize,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },
    /// Input length is not valid for base64.
    #[snafu(display("invalid base64 length: {length}"))]
    InvalidLength {
        /// The length of the input.
        length: usize,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },
    /// Padding is malformed or absent where required.
    #[snafu(display("invalid base64 padding"))]
    InvalidPadding {
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },
}

/// Standard base64 alphabet (with `+` and `/`).
const STANDARD_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// URL-safe base64 alphabet (with `-` and `_`).
const URL_SAFE_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Encode bytes to standard base64 (with `+`, `/`, and `=` padding).
#[must_use]
pub fn encode(input: &[u8]) -> String {
    encode_with_alphabet(input, STANDARD_ALPHABET, true)
}

/// Decode standard base64 (with `+`, `/`, `=` padding).
///
/// # Errors
///
/// Returns [`DecodeError`] if the input contains invalid characters,
/// has an invalid length, or has malformed padding.
pub fn decode(input: &str) -> Result<Vec<u8>, DecodeError> {
    decode_inner(input, false, true)
}

/// Encode bytes to URL-safe base64 (with `-`, `_`, no padding).
///
/// Used for JWT segments and PKCE verifiers.
#[must_use]
pub fn encode_url_safe_no_pad(input: &[u8]) -> String {
    encode_with_alphabet(input, URL_SAFE_ALPHABET, false)
}

/// Decode URL-safe base64 (with `-`, `_`, no padding required).
///
/// Leniently accepts `+` and `/` as aliases for `-` and `_`, and strips
/// any trailing `=` padding, so callers that receive mildly malformed
/// inputs do not fail unnecessarily.
///
/// # Errors
///
/// Returns [`DecodeError`] if the input contains invalid characters
/// or has an invalid length.
pub fn decode_url_safe_no_pad(input: &str) -> Result<Vec<u8>, DecodeError> {
    decode_inner(input, true, false)
}

/// Encode with a given alphabet and optional padding.
fn encode_with_alphabet(input: &[u8], alphabet: &[u8; 64], pad: bool) -> String {
    let mut out = String::with_capacity(input.len().saturating_mul(4).div_ceil(3));

    let mut chunks = input.chunks_exact(3);
    for chunk in &mut chunks {
        let [b0, b1, b2] = *chunk else {
            #[expect(
                clippy::unreachable,
                reason = "chunks_exact(3) yields only full-length chunks; remainder is handled separately"
            )]
            {
                unreachable!("chunks_exact(3) chunk must have len 3")
            }
        };
        let b = (u32::from(b0) << 16) | (u32::from(b1) << 8) | u32::from(b2);
        out.push(sextet_to_char(alphabet, b >> 18));
        out.push(sextet_to_char(alphabet, b >> 12));
        out.push(sextet_to_char(alphabet, b >> 6));
        out.push(sextet_to_char(alphabet, b));
    }

    let remainder = chunks.remainder();
    match *remainder {
        [] => {}
        [b0] => {
            let b = u32::from(b0) << 16;
            out.push(sextet_to_char(alphabet, b >> 18));
            out.push(sextet_to_char(alphabet, b >> 12));
            if pad {
                out.push('=');
                out.push('=');
            }
        }
        [b0, b1] => {
            let b = (u32::from(b0) << 16) | (u32::from(b1) << 8);
            out.push(sextet_to_char(alphabet, b >> 18));
            out.push(sextet_to_char(alphabet, b >> 12));
            out.push(sextet_to_char(alphabet, b >> 6));
            if pad {
                out.push('=');
            }
        }
        #[expect(
            clippy::unreachable,
            reason = "chunks_exact(3) guarantees remainder().len() ∈ {0, 1, 2}; only 3 arms reachable"
        )]
        _ => unreachable!("chunks_exact(3) remainder cannot exceed 2"),
    }

    out
}

/// Map a 6-bit sextet to a character in the given alphabet.
fn sextet_to_char(alphabet: &[u8; 64], six_bits: u32) -> char {
    #[expect(
        clippy::as_conversions,
        reason = "six_bits & 0x3F is always 0-63, fits in usize"
    )]
    let idx = (six_bits & 0x3F) as usize;
    // INVARIANT: `six_bits & 0x3F` is always 0-63, so the index is always in bounds.
    #[expect(
        clippy::indexing_slicing,
        reason = "six_bits & 0x3F is always in 0..64"
    )]
    char::from(alphabet[idx])
}

/// Decode base64 with configurable alphabet and padding requirements.
fn decode_inner(
    input: &str,
    allow_url_safe: bool,
    require_pad: bool,
) -> Result<Vec<u8>, DecodeError> {
    let bytes = input.as_bytes();

    // WHY: split data from trailing '=' padding before decoding.
    let data_end = bytes.iter().rposition(|&b| b != b'=').map_or(0, |i| i + 1);
    let padding_len = bytes.len().saturating_sub(data_end);

    if require_pad {
        if !bytes.len().is_multiple_of(4) {
            return InvalidLengthSnafu {
                length: bytes.len(),
            }
            .fail();
        }
        if padding_len > 2 {
            return InvalidPaddingSnafu.fail();
        }
    } else if data_end % 4 == 1 {
        // WHY: a no-pad input with length ≡ 1 (mod 4) cannot be valid base64.
        return InvalidLengthSnafu { length: data_end }.fail();
    }

    let mut out = Vec::with_capacity(data_end.saturating_mul(3).div_ceil(4));
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;

    for (pos, &b) in bytes.iter().enumerate().take(data_end) {
        let v = char_to_sextet(b, allow_url_safe).ok_or_else(|| {
            InvalidCharSnafu {
                ch: char::from(b),
                position: pos,
            }
            .build()
        })?;
        buf = (buf << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((buf >> bits) & 0xFF).unwrap_or(0));
        }
    }

    // INVARIANT: leftover bits in the final sextet must be zero.
    if bits > 0 {
        let mask = (1u32 << bits) - 1;
        if (buf & mask) != 0 {
            return InvalidPaddingSnafu.fail();
        }
    }

    Ok(out)
}

/// Map a single base64 character to its 6-bit value.
fn char_to_sextet(b: u8, allow_url_safe: bool) -> Option<u8> {
    match b {
        b'A'..=b'Z' => Some(b - b'A'),
        b'a'..=b'z' => Some(b - b'a' + 26),
        b'0'..=b'9' => Some(b - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        b'-' if allow_url_safe => Some(62),
        b'_' if allow_url_safe => Some(63),
        _ => None,
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn rfc4648_empty() {
        assert_eq!(encode(b""), "");
        assert_eq!(decode("").unwrap(), b"");
    }

    #[test]
    fn rfc4648_f() {
        assert_eq!(encode(b"f"), "Zg==");
        assert_eq!(decode("Zg==").unwrap(), b"f");
    }

    #[test]
    fn rfc4648_fo() {
        assert_eq!(encode(b"fo"), "Zm8=");
        assert_eq!(decode("Zm8=").unwrap(), b"fo");
    }

    #[test]
    fn rfc4648_foo() {
        assert_eq!(encode(b"foo"), "Zm9v");
        assert_eq!(decode("Zm9v").unwrap(), b"foo");
    }

    #[test]
    fn rfc4648_foob() {
        assert_eq!(encode(b"foob"), "Zm9vYg==");
        assert_eq!(decode("Zm9vYg==").unwrap(), b"foob");
    }

    #[test]
    fn rfc4648_fooba() {
        assert_eq!(encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(decode("Zm9vYmE=").unwrap(), b"fooba");
    }

    #[test]
    fn rfc4648_foobar() {
        assert_eq!(encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(decode("Zm9vYmFy").unwrap(), b"foobar");
    }

    #[test]
    fn url_safe_empty() {
        assert_eq!(encode_url_safe_no_pad(b""), "");
        assert_eq!(decode_url_safe_no_pad("").unwrap(), b"");
    }

    #[test]
    fn url_safe_f() {
        assert_eq!(encode_url_safe_no_pad(b"f"), "Zg");
        assert_eq!(decode_url_safe_no_pad("Zg").unwrap(), b"f");
    }

    #[test]
    fn url_safe_fo() {
        assert_eq!(encode_url_safe_no_pad(b"fo"), "Zm8");
        assert_eq!(decode_url_safe_no_pad("Zm8").unwrap(), b"fo");
    }

    #[test]
    fn url_safe_foo() {
        assert_eq!(encode_url_safe_no_pad(b"foo"), "Zm9v");
        assert_eq!(decode_url_safe_no_pad("Zm9v").unwrap(), b"foo");
    }

    #[test]
    fn url_safe_foob() {
        assert_eq!(encode_url_safe_no_pad(b"foob"), "Zm9vYg");
        assert_eq!(decode_url_safe_no_pad("Zm9vYg").unwrap(), b"foob");
    }

    #[test]
    fn url_safe_fooba() {
        assert_eq!(encode_url_safe_no_pad(b"fooba"), "Zm9vYmE");
        assert_eq!(decode_url_safe_no_pad("Zm9vYmE").unwrap(), b"fooba");
    }

    #[test]
    fn url_safe_foobar() {
        assert_eq!(encode_url_safe_no_pad(b"foobar"), "Zm9vYmFy");
        assert_eq!(decode_url_safe_no_pad("Zm9vYmFy").unwrap(), b"foobar");
    }

    #[test]
    fn url_safe_replaces_special_chars() {
        // Bytes that would produce + or / in standard base64.
        let input: &[u8] = &[0xfb, 0xff, 0xfe];
        let encoded = encode_url_safe_no_pad(input);
        assert!(!encoded.contains('+'));
        assert!(!encoded.contains('/'));
        assert!(!encoded.contains('='));
        assert_eq!(decode_url_safe_no_pad(&encoded).unwrap(), input);
    }

    #[test]
    fn url_safe_lenient_with_standard_chars() {
        // URL-safe decoder should accept + and / as well.
        let input = b"test";
        let encoded = encode(input);
        assert_eq!(decode_url_safe_no_pad(&encoded).unwrap(), input);
    }

    #[test]
    fn url_safe_lenient_with_padding() {
        // URL-safe decoder should strip trailing = padding.
        assert_eq!(decode_url_safe_no_pad("Zg==").unwrap(), b"f");
        assert_eq!(decode_url_safe_no_pad("Zm8=").unwrap(), b"fo");
    }

    #[test]
    fn url_safe_known_jwt_header() {
        let decoded = decode_url_safe_no_pad("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9").unwrap();
        assert_eq!(decoded, br#"{"alg":"HS256","typ":"JWT"}"#);
    }

    #[test]
    fn decode_invalid_char() {
        // Valid length (8), invalid character in the middle.
        assert!(matches!(
            decode("Zm9vYg@="),
            Err(DecodeError::InvalidChar { .. })
        ));
        // Valid length for no-pad (7), invalid character at end.
        assert!(matches!(
            decode_url_safe_no_pad("Zm9vYg@"),
            Err(DecodeError::InvalidChar { .. })
        ));
    }

    #[test]
    fn decode_wrong_length() {
        // "a" is 1 char — not a multiple of 4 for standard, and 1 mod 4 for no-pad.
        assert!(matches!(
            decode("a"),
            Err(DecodeError::InvalidLength { .. })
        ));
        assert!(matches!(
            decode_url_safe_no_pad("a"),
            Err(DecodeError::InvalidLength { .. })
        ));
    }

    #[test]
    fn decode_bad_padding() {
        // "abc==" is 5 chars — not a multiple of 4.
        assert!(matches!(
            decode("abc=="),
            Err(DecodeError::InvalidLength { .. })
        ));
        // "abcde" is 5 chars — not a multiple of 4.
        assert!(matches!(
            decode("abcde"),
            Err(DecodeError::InvalidLength { .. })
        ));
        // "ab==" has 2 data chars; the 4 trailing bits of the second char must be zero.
        // 'b' = 27 = 0b011011; last 4 bits = 0b1011 = 11 ≠ 0, so InvalidPadding.
        assert!(matches!(
            decode("ab=="),
            Err(DecodeError::InvalidPadding { .. })
        ));
    }

    #[test]
    fn decode_url_safe_bad_length() {
        // 1 mod 4 is always invalid for no-pad.
        assert!(matches!(
            decode_url_safe_no_pad("a"),
            Err(DecodeError::InvalidLength { .. })
        ));
        // 2 chars with non-zero leftover bits.
        assert!(matches!(
            decode_url_safe_no_pad("ab"),
            Err(DecodeError::InvalidPadding { .. })
        ));
    }

    #[test]
    fn roundtrip_standard() {
        let input = b"The quick brown fox jumps over the lazy dog.";
        let encoded = encode(input);
        let decoded = decode(&encoded).unwrap();
        assert_eq!(decoded, input);
    }

    #[test]
    fn roundtrip_url_safe() {
        let input = b"The quick brown fox jumps over the lazy dog.";
        let encoded = encode_url_safe_no_pad(input);
        let decoded = decode_url_safe_no_pad(&encoded).unwrap();
        assert_eq!(decoded, input);
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "proptest assertions")]
mod proptests {
    use proptest::prelude::*;

    use super::*;

    proptest! {
        #[test]
        fn prop_roundtrip_standard(data: Vec<u8>) {
            let encoded = encode(&data);
            let decoded = decode(&encoded).unwrap();
            prop_assert_eq!(decoded, data);
        }

        #[test]
        fn prop_roundtrip_url_safe(data: Vec<u8>) {
            let encoded = encode_url_safe_no_pad(&data);
            let decoded = decode_url_safe_no_pad(&encoded).unwrap();
            prop_assert_eq!(decoded, data);
        }
    }
}
