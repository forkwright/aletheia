//! Shared big-endian/zero-padded key encoding for fjall-backed stores.
//!
//! Both [`crate::uncertainty`] and [`crate::competence`] key their fjall
//! partitions on a zero-padded decimal sequence (for lexicographic ordering)
//! plus a big-endian `u64` counter value. This module owns that codec so the
//! two stores cannot drift on it; each keeps its own `SEQ_WIDTH` constant
//! since the width is part of that store's on-disk key schema.

/// Format a `u64` as a zero-padded decimal string `width` characters wide, so
/// lexicographic ordering of the resulting keys matches numeric ordering.
pub(crate) fn pad_u64(v: u64, width: usize) -> String {
    format!("{v:0>width$}")
}

/// Decode a big-endian `u64` from the first 8 bytes of `bytes`, defaulting to
/// `0` if fewer than 8 bytes are available.
pub(crate) fn decode_u64(bytes: &[u8]) -> u64 {
    let arr: [u8; 8] = bytes
        .get(..8)
        .and_then(|s| s.try_into().ok())
        .unwrap_or([0u8; 8]);
    u64::from_be_bytes(arr)
}

/// Encode a `u64` as big-endian bytes.
pub(crate) fn encode_u64(v: u64) -> [u8; 8] {
    v.to_be_bytes()
}
