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

/// Decode a base64url-encoded string (no padding required) into raw bytes.
///
/// WHY: extracts JWT payload segments to read `exp` claims without pulling in a
/// dedicated crate for this ~30-line function. Base64url differs from standard
/// Base64 only in the `+`/`-` and `/`/`_` substitutions and the omission of `=` padding.
pub(crate) fn base64url_decode(s: &str) -> Option<Vec<u8>> {
    /// Map a single base64url character to its 6-bit value.
    fn char_val(b: u8) -> Option<u8> {
        match b {
            b'A'..=b'Z' => Some(b - b'A'),
            b'a'..=b'z' => Some(b - b'a' + 26),
            b'0'..=b'9' => Some(b - b'0' + 52),
            b'-' | b'+' => Some(62),
            b'_' | b'/' => Some(63),
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
        let v = char_val(b)?;
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
}
