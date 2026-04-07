//! Hex encoding utilities.
//!
//! Provides a simple, allocation-efficient hex encoder for byte slices.

const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";

/// Encode a byte slice as a lowercase hexadecimal string.
///
/// # Example
///
/// ```
/// use aletheia_koina::hex::encode;
///
/// let encoded = encode(&[0xde, 0xad, 0xbe, 0xef]);
/// assert_eq!(encoded, "deadbeef");
/// ```
#[must_use]
pub fn encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        // b >> 4 is 0..=15 and b & 0x0f is 0..=15; the array has exactly 16 elements
        #[expect(
            clippy::indexing_slicing,
            reason = "nibble value 0..=15 always indexes into 16-element array"
        )]
        s.push(char::from(HEX_CHARS[usize::from(b >> 4)]));
        #[expect(
            clippy::indexing_slicing,
            reason = "nibble value 0..=15 always indexes into 16-element array"
        )]
        s.push(char::from(HEX_CHARS[usize::from(b & 0x0f)]));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_empty() {
        assert_eq!(encode(&[]), "");
    }

    #[test]
    fn encode_single_byte() {
        assert_eq!(encode(&[0x00]), "00");
        assert_eq!(encode(&[0x0f]), "0f");
        assert_eq!(encode(&[0xf0]), "f0");
        assert_eq!(encode(&[0xff]), "ff");
    }

    #[test]
    fn encode_multiple_bytes() {
        assert_eq!(encode(&[0xde, 0xad, 0xbe, 0xef]), "deadbeef");
        assert_eq!(encode(&[0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef]), "0123456789abcdef");
    }
}
