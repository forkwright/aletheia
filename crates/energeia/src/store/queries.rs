//! Prefix scan query implementations for the energeia store.
//!
//! Each query function takes a fjall `Keyspace` reference and returns
//! deserialized records filtered and ordered by key prefix.

#[cfg(feature = "storage-fjall")]
use crate::error::{self, Result};
#[cfg(feature = "storage-fjall")]
use crate::store::records::{
    CiValidationRecord, DispatchRecord, LessonRecord, ObservationRecord, QaVerdictRecord,
    SessionRecord,
};
#[cfg(feature = "storage-fjall")]
use crate::store::schema;

#[cfg(feature = "storage-fjall")]
fn prefix_upper_bound(prefix: &str) -> String {
    let mut bytes = prefix.as_bytes().to_vec();
    if let Some(last) = bytes.last_mut() {
        *last = last.saturating_add(1);
    }
    String::from_utf8(bytes).unwrap_or_else(|_| prefix.to_owned())
}

#[cfg(feature = "storage-fjall")]
fn ulid_floor_for_millisecond(ms: i64) -> String {
    let clamped_ms = u64::try_from(ms).unwrap_or(0).min((1_u64 << 48) - 1);
    let value = u128::from(clamped_ms) << 80;
    koina::ulid::Ulid::from_u128(value).to_string()
}

/// Deserialize a `MessagePack` value from a fjall value slice.
#[cfg(feature = "storage-fjall")]
pub(crate) fn deserialize_value<T: serde::de::DeserializeOwned>(value: &[u8]) -> Result<T> {
    rmp_serde::from_slice(value).map_err(|e| {
        error::SerializationSnafu {
            message: format!("deserialize: {e}"),
        }
        .build()
    })
}

/// Scan a fjall keyspace by prefix, deserializing each value and collecting
/// results that pass an optional filter.
///
/// # Arguments
///
/// * `keyspace` — the fjall keyspace to scan
/// * `prefix` — key prefix bytes to scan
/// * `context` — human-readable context for error messages (e.g. "session prefix scan")
/// * `limit` — maximum number of results to return (`usize::MAX` for no limit)
/// * `filter` — predicate applied after deserialization; returning `false` skips the record
#[cfg(feature = "storage-fjall")]
fn prefix_scan<T: serde::de::DeserializeOwned>(
    keyspace: &fjall::Keyspace,
    prefix: &[u8],
    context: &str,
    limit: usize,
    filter: impl Fn(&T) -> bool,
) -> Result<Vec<T>> {
    let mut results = Vec::new();
    for guard in keyspace.prefix(prefix) {
        if results.len() >= limit {
            break;
        }
        let (_key, value) = guard.into_inner().map_err(|e| {
            error::StoreSnafu {
                message: format!("{context}: {e}"),
            }
            .build()
        })?;
        let record = deserialize_value::<T>(&value)?;
        if filter(&record) {
            results.push(record);
        }
    }
    Ok(results)
}

/// Scan a fjall keyspace by prefix from newest key to oldest.
#[cfg(feature = "storage-fjall")]
fn prefix_scan_reverse<T: serde::de::DeserializeOwned>(
    keyspace: &fjall::Keyspace,
    prefix: &[u8],
    context: &str,
    limit: usize,
) -> Result<Vec<T>> {
    let mut results = Vec::new();
    for guard in keyspace.prefix(prefix).rev() {
        if results.len() >= limit {
            break;
        }
        let (_key, value) = guard.into_inner().map_err(|e| {
            error::StoreSnafu {
                message: format!("{context}: {e}"),
            }
            .build()
        })?;
        results.push(deserialize_value::<T>(&value)?);
    }
    Ok(results)
}

#[cfg(feature = "storage-fjall")]
fn range_scan<T: serde::de::DeserializeOwned>(
    keyspace: &fjall::Keyspace,
    start: &[u8],
    end: &[u8],
    context: &str,
    limit: usize,
    filter: impl Fn(&T) -> bool,
) -> Result<Vec<T>> {
    let mut results = Vec::new();
    for guard in keyspace.range(start..end) {
        if results.len() >= limit {
            break;
        }
        let (_key, value) = guard.into_inner().map_err(|e| {
            error::StoreSnafu {
                message: format!("{context}: {e}"),
            }
            .build()
        })?;
        let record = deserialize_value::<T>(&value)?;
        if filter(&record) {
            results.push(record);
        }
    }
    Ok(results)
}

/// Collect all sessions for a given dispatch via prefix scan.
#[cfg(feature = "storage-fjall")]
pub(crate) fn list_sessions_for_dispatch(
    keyspace: &fjall::Keyspace,
    dispatch_id: &crate::store::records::DispatchId,
) -> Result<Vec<SessionRecord>> {
    let prefix = schema::session_prefix_for_dispatch(dispatch_id);
    prefix_scan(
        keyspace,
        prefix.as_bytes(),
        "session prefix scan",
        usize::MAX,
        |_: &SessionRecord| true,
    )
}

/// Query lessons with optional source filter, returning up to `limit` results.
///
/// Results are ordered by key (source + timestamp ascending).
#[cfg(feature = "storage-fjall")]
pub(crate) fn query_lessons(
    keyspace: &fjall::Keyspace,
    source: Option<&str>,
    category: Option<&str>,
    project: Option<&str>,
    limit: usize,
) -> Result<Vec<LessonRecord>> {
    let owned_prefix: String;
    let prefix_bytes: &[u8] = match source {
        Some(s) => {
            owned_prefix = schema::lesson_prefix_for_source(s);
            owned_prefix.as_bytes()
        }
        None => schema::lesson_prefix().as_bytes(),
    };

    prefix_scan(
        keyspace,
        prefix_bytes,
        "lesson prefix scan",
        limit,
        |record: &LessonRecord| {
            if category.is_some_and(|cat| record.category != cat) {
                return false;
            }
            if project.is_some_and(|proj| record.project.as_deref() != Some(proj)) {
                return false;
            }
            true
        },
    )
}

/// Query observations with optional project filter and day window.
///
/// Results are ordered by timestamp ascending (key order).
#[cfg(feature = "storage-fjall")]
pub(crate) fn query_observations(
    keyspace: &fjall::Keyspace,
    project: Option<&str>,
    days: Option<u32>,
    limit: usize,
) -> Result<Vec<ObservationRecord>> {
    let prefix_bytes = schema::observation_prefix().as_bytes();

    let cutoff_ms: Option<i64> = days.map(|d| {
        let now = jiff::Timestamp::now();
        let span = jiff::SignedDuration::from_hours(i64::from(d) * 24);
        // WHY: subtraction of a bounded span from current time cannot fail
        // for any realistic day count.
        #[expect(
            clippy::expect_used,
            reason = "bounded subtraction from now is infallible for realistic day counts"
        )]
        let cutoff = now.checked_sub(span).expect("timestamp subtraction"); // INVARIANT: span = d*24h from current time; subtraction always valid
        cutoff.as_millisecond()
    });

    prefix_scan(
        keyspace,
        prefix_bytes,
        "observation prefix scan",
        limit,
        |record: &ObservationRecord| {
            if cutoff_ms.is_some_and(|cutoff| record.created_at.as_millisecond() < cutoff) {
                return false;
            }
            if project.is_some_and(|proj| record.project != proj) {
                return false;
            }
            true
        },
    )
}

/// Collect all dispatch records via prefix scan.
///
/// Results are ordered by ULID (time-ascending). Use `limit` to cap memory
/// usage; pass `usize::MAX` for no limit.
#[cfg(feature = "storage-fjall")]
pub(crate) fn list_dispatches(
    keyspace: &fjall::Keyspace,
    limit: usize,
) -> Result<Vec<crate::store::records::DispatchRecord>> {
    let prefix_bytes = schema::dispatch_prefix().as_bytes();
    prefix_scan(
        keyspace,
        prefix_bytes,
        "dispatch prefix scan",
        limit,
        |_: &DispatchRecord| true,
    )
}

/// Collect dispatch records whose creation time is inside the optional window.
#[cfg(feature = "storage-fjall")]
pub(crate) fn list_dispatches_since(
    keyspace: &fjall::Keyspace,
    cutoff_ms: Option<i64>,
    limit: usize,
) -> Result<Vec<crate::store::records::DispatchRecord>> {
    let prefix = schema::dispatch_prefix();
    let start = cutoff_ms.map_or_else(
        || prefix.to_owned(),
        |cutoff| format!("{prefix}{}", ulid_floor_for_millisecond(cutoff)),
    );
    let end = prefix_upper_bound(prefix);
    range_scan(
        keyspace,
        start.as_bytes(),
        end.as_bytes(),
        "dispatch range scan",
        limit,
        |record: &DispatchRecord| {
            cutoff_ms.is_none_or(|cutoff| record.created_at.as_millisecond() >= cutoff)
        },
    )
}

/// Collect newest dispatch records via reverse prefix scan.
///
/// Results are ordered by ULID descending. Use `limit` to cap memory usage.
#[cfg(feature = "storage-fjall")]
pub(crate) fn list_recent_dispatches(
    keyspace: &fjall::Keyspace,
    limit: usize,
) -> Result<Vec<crate::store::records::DispatchRecord>> {
    let prefix_bytes = schema::dispatch_prefix().as_bytes();
    prefix_scan_reverse(keyspace, prefix_bytes, "dispatch prefix scan", limit)
}

/// Collect all session records across all dispatches via prefix scan.
///
/// Results are ordered by `(dispatch_ulid, prompt_number)` (time-approximate
/// ascending). Use `limit` to cap memory usage.
#[cfg(feature = "storage-fjall")]
pub(crate) fn list_all_sessions(
    keyspace: &fjall::Keyspace,
    limit: usize,
) -> Result<Vec<SessionRecord>> {
    let prefix_bytes = schema::session_prefix().as_bytes();
    prefix_scan(
        keyspace,
        prefix_bytes,
        "session prefix scan",
        limit,
        |_: &SessionRecord| true,
    )
}

/// Collect session records whose creation time is inside the optional window.
#[cfg(feature = "storage-fjall")]
pub(crate) fn list_all_sessions_since(
    keyspace: &fjall::Keyspace,
    cutoff_ms: Option<i64>,
    limit: usize,
) -> Result<Vec<SessionRecord>> {
    let prefix = schema::session_prefix();
    let start = cutoff_ms.map_or_else(
        || prefix.to_owned(),
        |cutoff| format!("{prefix}{}:", ulid_floor_for_millisecond(cutoff)),
    );
    let end = prefix_upper_bound(prefix);
    range_scan(
        keyspace,
        start.as_bytes(),
        end.as_bytes(),
        "session range scan",
        limit,
        |record: &SessionRecord| {
            cutoff_ms.is_none_or(|cutoff| record.created_at.as_millisecond() >= cutoff)
        },
    )
}

/// Collect CI validation records whose validation time is inside the optional window.
#[cfg(feature = "storage-fjall")]
pub(crate) fn list_all_ci_validations_since(
    keyspace: &fjall::Keyspace,
    cutoff_ms: Option<i64>,
    limit: usize,
) -> Result<Vec<CiValidationRecord>> {
    let prefix_bytes = schema::ci_validation_prefix().as_bytes();
    prefix_scan(
        keyspace,
        prefix_bytes,
        "ci_validation prefix scan",
        limit,
        |record: &CiValidationRecord| {
            cutoff_ms.is_none_or(|cutoff| record.validated_at.as_millisecond() >= cutoff)
        },
    )
}

/// Collect CI validations for a given session via prefix scan.
#[cfg(feature = "storage-fjall")]
pub(crate) fn list_ci_validations_for_session(
    keyspace: &fjall::Keyspace,
    session_id: &crate::store::records::SessionId,
) -> Result<Vec<CiValidationRecord>> {
    let prefix = schema::ci_validation_prefix_for_session(session_id);
    prefix_scan(
        keyspace,
        prefix.as_bytes(),
        "ci_validation prefix scan",
        usize::MAX,
        |_: &CiValidationRecord| true,
    )
}

/// Collect QA verdict records whose record time is inside the optional window.
#[cfg(feature = "storage-fjall")]
pub(crate) fn list_all_qa_verdicts_since(
    keyspace: &fjall::Keyspace,
    cutoff_ms: Option<i64>,
    limit: usize,
) -> Result<Vec<QaVerdictRecord>> {
    let prefix_bytes = schema::qa_verdict_prefix().as_bytes();
    prefix_scan(
        keyspace,
        prefix_bytes,
        "qa_verdict prefix scan",
        limit,
        |record: &QaVerdictRecord| {
            cutoff_ms.is_none_or(|cutoff| record.recorded_at.as_millisecond() >= cutoff)
        },
    )
}

/// Collect QA verdict records for a given dispatch via prefix scan.
#[cfg(feature = "storage-fjall")]
pub(crate) fn list_qa_verdicts_for_dispatch(
    keyspace: &fjall::Keyspace,
    dispatch_id: &crate::store::records::DispatchId,
) -> Result<Vec<QaVerdictRecord>> {
    let prefix = schema::qa_verdict_prefix_for_dispatch(dispatch_id);
    prefix_scan(
        keyspace,
        prefix.as_bytes(),
        "qa_verdict prefix scan",
        usize::MAX,
        |_: &QaVerdictRecord| true,
    )
}
