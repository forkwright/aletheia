//! Provenance hashing for distillation reconstruction.
//!
//! [`crate::distill::DistillResult`] and [`crate::flush::FlushItem`] carry
//! provenance hashes and content-hashed message references, never raw
//! prompt or source-excerpt content.
//!
//! # Redaction policy
//!
//! A hash lets a later auditor confirm that a specific input produced a
//! specific output without persisting the conversation content itself in
//! the provenance record. The same content-hash-not-content pattern is
//! used elsewhere for evidence provenance (e.g. `mneme::finding::stable_hash`
//! in the prosoche self-audit framework). Reconstructing the original text
//! from a hash requires the source conversation transcript, which
//! [`message_ref`] and [`hash_str`] do not embed and do not make
//! recoverable. A provenance record is therefore safe to retain and share
//! more broadly than the transcript it describes: it proves what happened
//! without repeating what was said.
//!
//! This module has no memory of its own; every hash it returns is a pure
//! function of its input. [`crate::distill::verify_provenance`] re-derives
//! these values and confirms they match a stored record -- but it does so
//! from the exact pruned, post-similarity-filtering message set that was
//! actually distilled (see
//! [`crate::distill::DistillResult::source_message_ids`]), not from the
//! original conversation. Similarity pruning depends on hash-map iteration
//! order internal to its LSH bucketing and is not guaranteed to reduce an
//! identical conversation to an identical set twice, so a caller must
//! retain the pruned set itself rather than re-derive it from the
//! original messages.

use sha2::{Digest, Sha256};

use hermeneus::types::Message;

/// Hash arbitrary content for provenance fingerprinting.
///
/// Returns `"sha256:<hex>"`. Deterministic: identical input always produces
/// identical output, so two distillation runs over the same input can be
/// compared without diffing the raw text.
#[must_use]
pub fn hash_str(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2 + 7);
    out.push_str("sha256:");
    for byte in digest {
        out.push(nibble_to_hex(byte >> 4));
        out.push(nibble_to_hex(byte & 0x0F));
    }
    out
}

// WHY nibble lookup rather than `format!("{byte:02x}")`: appending a `format!`
// to an existing String allocates a throwaway String per byte, which
// `clippy::format_push_string` denies. This is the shape the rest of the
// workspace already uses for the same job -- see `hex_encode` in
// `nous/src/hooks/builtins/correction.rs` and `hex_lower` in
// `aletheia/src/dispatch.rs`.
//
// NOTE(#6789): those, plus `daemon/src/runner/output.rs` and
// `eval/src/provenance.rs`, are four private copies of this function; this is
// a fifth. A shared owner is tracked separately rather than adding a
// cross-crate dependency from here.
fn nibble_to_hex(n: u8) -> char {
    if n < 10 {
        char::from(b'0' + n)
    } else {
        char::from(b'a' + (n - 10))
    }
}

/// A stable per-message reference for provenance tracking.
///
/// [`Message`] carries no identifier of its own, so this hashes the
/// message's canonical JSON serialization (role, content, and
/// cache-breakpoint flag). Two messages with identical content produce the
/// same reference -- this is a content fingerprint, not a globally unique
/// ID, and does not distinguish otherwise-identical messages at different
/// points in a conversation.
#[must_use]
pub fn message_ref(message: &Message) -> String {
    let canonical = serde_json::to_string(message).unwrap_or_default();
    format!("msg:{}", hash_str(&canonical))
}

/// References for a slice of messages, in order.
#[must_use]
pub fn message_refs(messages: &[Message]) -> Vec<String> {
    messages.iter().map(message_ref).collect()
}

/// Number of hex characters of the digest kept by [`message_ref_short`].
const SHORT_REF_LEN: usize = 8;

/// Short prefix of [`message_ref`]'s digest, for embedding inline in
/// formatted prompt text where a full 71-character reference would cost
/// meaningful token budget.
///
/// Collisions are vanishingly unlikely at the per-session message counts
/// this crate operates on (a handful to a few hundred), not at the scale a
/// content-addressed store needs to defend against -- [`message_ref`]
/// keeps the full digest for provenance records that must be
/// collision-resistant across a much larger population.
#[must_use]
pub fn message_ref_short(message: &Message) -> String {
    let full = message_ref(message);
    full.rsplit(':').next().map_or_else(
        || full.clone(),
        |hex| hex.chars().take(SHORT_REF_LEN).collect(),
    )
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    reason = "test: vec indices are valid after asserting len"
)]
mod tests {
    use super::*;
    use hermeneus::types::{Content, Role};

    #[test]
    fn hash_str_is_deterministic() {
        assert_eq!(hash_str("hello"), hash_str("hello"));
    }

    #[test]
    fn hash_str_differs_on_different_input() {
        assert_ne!(hash_str("hello"), hash_str("world"));
    }

    #[test]
    fn hash_str_has_sha256_prefix() {
        assert!(hash_str("anything").starts_with("sha256:"));
    }

    fn msg(role: Role, text: &str) -> Message {
        Message {
            role,
            content: Content::Text(text.to_owned()),
            cache_breakpoint: false,
        }
    }

    #[test]
    fn message_ref_is_deterministic() {
        let a = msg(Role::User, "hello");
        let b = msg(Role::User, "hello");
        assert_eq!(message_ref(&a), message_ref(&b));
    }

    #[test]
    fn message_ref_differs_on_different_content() {
        let a = msg(Role::User, "hello");
        let b = msg(Role::User, "goodbye");
        assert_ne!(message_ref(&a), message_ref(&b));
    }

    #[test]
    fn message_ref_differs_on_different_role() {
        let a = msg(Role::User, "hello");
        let b = msg(Role::Assistant, "hello");
        assert_ne!(
            message_ref(&a),
            message_ref(&b),
            "role is part of the canonical serialization"
        );
    }

    #[test]
    fn message_ref_never_contains_raw_content() {
        let m = msg(Role::User, "a very specific secret string");
        let reference = message_ref(&m);
        assert!(
            !reference.contains("secret"),
            "reference must be a hash, not the content: {reference}"
        );
    }

    #[test]
    fn message_ref_short_is_a_prefix_of_the_full_digest() {
        let m = msg(Role::User, "hello");
        let full = message_ref(&m);
        let short = message_ref_short(&m);
        assert_eq!(short.len(), SHORT_REF_LEN);
        assert!(
            full.ends_with(&short) || full.contains(&short),
            "short ref {short} must derive from full ref {full}"
        );
    }

    #[test]
    fn message_ref_short_differs_on_different_content() {
        let a = msg(Role::User, "hello");
        let b = msg(Role::User, "goodbye");
        assert_ne!(message_ref_short(&a), message_ref_short(&b));
    }

    #[test]
    fn message_refs_preserves_order() {
        let messages = vec![msg(Role::User, "first"), msg(Role::Assistant, "second")];
        let refs = message_refs(&messages);
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0], message_ref(&messages[0]));
        assert_eq!(refs[1], message_ref(&messages[1]));
    }
}
