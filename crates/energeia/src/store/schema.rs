// WHY: encoding primitives are building blocks — some are only consumed by
// tests or by cfg(feature = "storage-fjall") query code today, but all will
// be needed as the store API surface grows.
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "schema primitives used across feature gates and tests"
    )
)]

//! Key encoding and decoding for energeia's fjall key-value schema.
//!
//! All keys use a string prefix followed by a colon separator for efficient
//! prefix scans. ULIDs provide time-sortable ordering within each prefix.
//!
//! Key layout:
//! ```text
//! dispatch:{ulid}                          -> DispatchRecord
//! session:{dispatch_ulid}:{prompt_no:08}   -> SessionRecord
//! lesson:{source}:{timestamp_ms}           -> LessonRecord
//! observation:{timestamp_ms}:{ulid}        -> ObservationRecord
//! ci_validation:{session_id}:{check_name}  -> CiValidationRecord
//! ```

use crate::store::records::{DispatchId, SessionId};

/// Key prefix for dispatch records.
const PREFIX_DISPATCH: &str = "dispatch:";
/// Key prefix for session records.
const PREFIX_SESSION: &str = "session:";
/// Key prefix for lesson records.
const PREFIX_LESSON: &str = "lesson:";
/// Key prefix for observation records.
const PREFIX_OBSERVATION: &str = "observation:";
/// Key prefix for CI validation records.
const PREFIX_CI_VALIDATION: &str = "ci_validation:";

// ---------------------------------------------------------------------------
// Dispatch keys
// ---------------------------------------------------------------------------

/// Encode a dispatch key: `dispatch:{ulid}`.
#[must_use]
pub(crate) fn dispatch_key(id: &DispatchId) -> String {
    format!("{PREFIX_DISPATCH}{}", id.as_str())
}

/// Prefix for scanning all dispatches (time-sorted by ULID).
#[must_use]
#[expect(dead_code, reason = "scan prefix infrastructure for future store queries")]
pub(crate) fn dispatch_prefix() -> &'static str {
    PREFIX_DISPATCH
}

/// Extract the `DispatchId` from a dispatch key.
#[must_use]
pub(crate) fn decode_dispatch_key(key: &[u8]) -> Option<DispatchId> {
    let s = std::str::from_utf8(key).ok()?;
    let id_str = s.strip_prefix(PREFIX_DISPATCH)?;
    Some(DispatchId::new(id_str))
}

// ---------------------------------------------------------------------------
// Session keys
// ---------------------------------------------------------------------------

/// Encode a session key: `session:{dispatch_ulid}:{prompt_no:08}`.
///
/// The prompt number is zero-padded to 8 digits for lexicographic ordering.
#[must_use]
pub(crate) fn session_key(dispatch_id: &DispatchId, prompt_number: u32) -> String {
    format!(
        "{PREFIX_SESSION}{}:{prompt_number:08}",
        dispatch_id.as_str()
    )
}

/// Prefix for scanning all sessions belonging to a dispatch.
#[must_use]
pub(crate) fn session_prefix_for_dispatch(dispatch_id: &DispatchId) -> String {
    format!("{PREFIX_SESSION}{}:", dispatch_id.as_str())
}

/// Prefix for scanning all sessions across all dispatches.
#[must_use]
#[expect(dead_code, reason = "scan prefix infrastructure for future store queries")]
pub(crate) fn session_prefix() -> &'static str {
    PREFIX_SESSION
}

/// Extract `(DispatchId, prompt_number)` from a session key.
#[must_use]
pub(crate) fn decode_session_key(key: &[u8]) -> Option<(DispatchId, u32)> {
    let s = std::str::from_utf8(key).ok()?;
    let rest = s.strip_prefix(PREFIX_SESSION)?;
    let (dispatch_str, prompt_str) = rest.rsplit_once(':')?;
    let prompt_number = prompt_str.parse().ok()?;
    Some((DispatchId::new(dispatch_str), prompt_number))
}

// ---------------------------------------------------------------------------
// Lesson keys
// ---------------------------------------------------------------------------

/// Encode a lesson key: `lesson:{source}:{timestamp_ms}:{ulid}`.
///
/// The ULID suffix ensures uniqueness when multiple lessons share the same
/// source and timestamp.
#[must_use]
pub(crate) fn lesson_key(source: &str, timestamp_ms: i64, ulid: &str) -> String {
    format!("{PREFIX_LESSON}{source}:{timestamp_ms:020}:{ulid}")
}

/// Prefix for scanning lessons by source.
#[must_use]
pub(crate) fn lesson_prefix_for_source(source: &str) -> String {
    format!("{PREFIX_LESSON}{source}:")
}

/// Prefix for scanning all lessons.
#[must_use]
#[expect(dead_code, reason = "scan prefix infrastructure for future store queries")]
pub(crate) fn lesson_prefix() -> &'static str {
    PREFIX_LESSON
}

// ---------------------------------------------------------------------------
// Observation keys
// ---------------------------------------------------------------------------

/// Encode an observation key: `observation:{timestamp_ms}:{ulid}`.
#[must_use]
pub(crate) fn observation_key(timestamp_ms: i64, ulid: &str) -> String {
    format!("{PREFIX_OBSERVATION}{timestamp_ms:020}:{ulid}")
}

/// Prefix for scanning all observations.
#[must_use]
#[expect(dead_code, reason = "scan prefix infrastructure for future store queries")]
pub(crate) fn observation_prefix() -> &'static str {
    PREFIX_OBSERVATION
}

// ---------------------------------------------------------------------------
// CI validation keys
// ---------------------------------------------------------------------------

/// Encode a CI validation key: `ci_validation:{session_id}:{check_name}`.
#[must_use]
pub(crate) fn ci_validation_key(session_id: &SessionId, check_name: &str) -> String {
    format!("{PREFIX_CI_VALIDATION}{}:{check_name}", session_id.as_str())
}

/// Prefix for scanning all CI validation records.
#[must_use]
#[expect(dead_code, reason = "scan prefix infrastructure for future store queries")]
pub(crate) fn ci_validation_prefix() -> &'static str {
    PREFIX_CI_VALIDATION
}

/// Prefix for scanning CI validations for a session.
#[must_use]
pub(crate) fn ci_validation_prefix_for_session(session_id: &SessionId) -> String {
    format!("{PREFIX_CI_VALIDATION}{}:", session_id.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_key_format() {
        let id = DispatchId::new("01JQXYZ123");
        assert_eq!(dispatch_key(&id), "dispatch:01JQXYZ123");
    }

    #[test]
    fn dispatch_key_roundtrip() {
        let id = DispatchId::new("01JQXYZ123");
        let key = dispatch_key(&id);
        let decoded = decode_dispatch_key(key.as_bytes());
        assert_eq!(decoded, Some(id));
    }

    #[test]
    fn session_key_format() {
        let dispatch_id = DispatchId::new("01JQXYZ123");
        assert_eq!(session_key(&dispatch_id, 1), "session:01JQXYZ123:00000001");
        assert_eq!(session_key(&dispatch_id, 42), "session:01JQXYZ123:00000042");
    }

    #[test]
    fn session_key_roundtrip() {
        let dispatch_id = DispatchId::new("01JQXYZ123");
        let prompt = 7;
        let key = session_key(&dispatch_id, prompt);
        let decoded = decode_session_key(key.as_bytes());
        assert_eq!(decoded, Some((dispatch_id, prompt)));
    }

    #[test]
    fn session_prefix_scopes_to_dispatch() {
        let dispatch_id = DispatchId::new("01JQXYZ123");
        let prefix = session_prefix_for_dispatch(&dispatch_id);
        let key_a = session_key(&dispatch_id, 1);
        let key_b = session_key(&dispatch_id, 99);
        assert!(key_a.starts_with(&prefix));
        assert!(key_b.starts_with(&prefix));

        let other_dispatch = DispatchId::new("01JQOTHER");
        let key_c = session_key(&other_dispatch, 1);
        assert!(!key_c.starts_with(&prefix));
    }

    #[test]
    fn lesson_key_format() {
        let key = lesson_key("steward", 1_700_000_000_000, "01JQABC");
        assert_eq!(key, "lesson:steward:00000001700000000000:01JQABC");
    }

    #[test]
    fn lesson_prefix_scopes_to_source() {
        let prefix = lesson_prefix_for_source("qa");
        let key = lesson_key("qa", 1_700_000_000_000, "01JQABC");
        assert!(key.starts_with(&prefix));

        let other = lesson_key("steward", 1_700_000_000_000, "01JQABC");
        assert!(!other.starts_with(&prefix));
    }

    #[test]
    fn observation_key_format() {
        let key = observation_key(1_700_000_000_000, "01JQOBS001");
        assert_eq!(key, "observation:00000001700000000000:01JQOBS001");
    }

    #[test]
    fn ci_validation_key_format() {
        let session_id = SessionId::new("01JQSESS01");
        let key = ci_validation_key(&session_id, "clippy");
        assert_eq!(key, "ci_validation:01JQSESS01:clippy");
    }

    #[test]
    fn ci_validation_prefix_scopes_to_session() {
        let session_id = SessionId::new("01JQSESS01");
        let prefix = ci_validation_prefix_for_session(&session_id);
        let key = ci_validation_key(&session_id, "clippy");
        assert!(key.starts_with(&prefix));

        let other_session = SessionId::new("01JQOTHER");
        let other_key = ci_validation_key(&other_session, "clippy");
        assert!(!other_key.starts_with(&prefix));
    }

    #[test]
    fn session_keys_sort_by_prompt_number() {
        let dispatch_id = DispatchId::new("01JQXYZ123");
        let k1 = session_key(&dispatch_id, 1);
        let k2 = session_key(&dispatch_id, 2);
        let k10 = session_key(&dispatch_id, 10);
        assert!(k1 < k2);
        assert!(k2 < k10);
    }

    #[test]
    fn observation_keys_sort_by_timestamp() {
        let k1 = observation_key(1_000, "01AAAA");
        let k2 = observation_key(2_000, "01AAAA");
        assert!(k1 < k2);
    }
}
