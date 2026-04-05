//! Prefix scan query implementations for the energeia store.
//!
//! Each query function takes a fjall `Keyspace` reference and returns
//! deserialized records filtered and ordered by key prefix.

#[cfg(feature = "storage-fjall")]
use crate::error::{self, Result};
#[cfg(feature = "storage-fjall")]
use crate::store::records::{
    CiValidationRecord, DispatchRecord, LessonRecord, ObservationRecord, SessionRecord,
};
#[cfg(feature = "storage-fjall")]
use crate::store::schema;

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

/// Collect all sessions for a given dispatch via prefix scan.
#[cfg(feature = "storage-fjall")]
pub(crate) fn list_sessions_for_dispatch(
    keyspace: &fjall::Keyspace,
    dispatch_id: &crate::store::records::DispatchId,
) -> Result<Vec<SessionRecord>> {
    let prefix = schema::session_prefix_for_dispatch(dispatch_id);
    let mut results = Vec::new();
    for guard in keyspace.prefix(prefix.as_bytes()) {
        let (_key, value) = guard.into_inner().map_err(|e| {
            error::StoreSnafu {
                message: format!("session prefix scan: {e}"),
            }
            .build()
        })?;
        results.push(deserialize_value::<SessionRecord>(&value)?);
    }
    Ok(results)
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

    let mut results = Vec::new();
    for guard in keyspace.prefix(prefix_bytes) {
        if results.len() >= limit {
            break;
        }
        let (_key, value) = guard.into_inner().map_err(|e| {
            error::StoreSnafu {
                message: format!("lesson prefix scan: {e}"),
            }
            .build()
        })?;
        let record = deserialize_value::<LessonRecord>(&value)?;

        if category.is_some_and(|cat| record.category != cat) {
            continue;
        }
        if project.is_some_and(|proj| record.project.as_deref() != Some(proj)) {
            continue;
        }

        results.push(record);
    }
    Ok(results)
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
        let cutoff = now.checked_sub(span).expect("timestamp subtraction");
        cutoff.as_millisecond()
    });

    let mut results = Vec::new();
    for guard in keyspace.prefix(prefix_bytes) {
        if results.len() >= limit {
            break;
        }
        let (_key, value) = guard.into_inner().map_err(|e| {
            error::StoreSnafu {
                message: format!("observation prefix scan: {e}"),
            }
            .build()
        })?;
        let record = deserialize_value::<ObservationRecord>(&value)?;

        if cutoff_ms.is_some_and(|cutoff| record.created_at.as_millisecond() < cutoff) {
            continue;
        }

        if project.is_some_and(|proj| record.project != proj) {
            continue;
        }

        results.push(record);
    }
    Ok(results)
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
    let mut results = Vec::new();
    for guard in keyspace.prefix(prefix_bytes) {
        if results.len() >= limit {
            break;
        }
        let (_key, value) = guard.into_inner().map_err(|e| {
            error::StoreSnafu {
                message: format!("dispatch prefix scan: {e}"),
            }
            .build()
        })?;
        results.push(deserialize_value::<DispatchRecord>(&value)?);
    }
    Ok(results)
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
    let mut results = Vec::new();
    for guard in keyspace.prefix(prefix_bytes) {
        if results.len() >= limit {
            break;
        }
        let (_key, value) = guard.into_inner().map_err(|e| {
            error::StoreSnafu {
                message: format!("session prefix scan: {e}"),
            }
            .build()
        })?;
        results.push(deserialize_value::<SessionRecord>(&value)?);
    }
    Ok(results)
}

/// Collect all CI validation records across all sessions via prefix scan.
///
/// Use `limit` to cap memory usage.
#[cfg(feature = "storage-fjall")]
pub(crate) fn list_all_ci_validations(
    keyspace: &fjall::Keyspace,
    limit: usize,
) -> Result<Vec<CiValidationRecord>> {
    let prefix_bytes = schema::ci_validation_prefix().as_bytes();
    let mut results = Vec::new();
    for guard in keyspace.prefix(prefix_bytes) {
        if results.len() >= limit {
            break;
        }
        let (_key, value) = guard.into_inner().map_err(|e| {
            error::StoreSnafu {
                message: format!("ci_validation prefix scan: {e}"),
            }
            .build()
        })?;
        results.push(deserialize_value::<CiValidationRecord>(&value)?);
    }
    Ok(results)
}

/// Collect CI validations for a given session via prefix scan.
#[cfg(feature = "storage-fjall")]
pub(crate) fn list_ci_validations_for_session(
    keyspace: &fjall::Keyspace,
    session_id: &crate::store::records::SessionId,
) -> Result<Vec<CiValidationRecord>> {
    let prefix = schema::ci_validation_prefix_for_session(session_id);
    let mut results = Vec::new();
    for guard in keyspace.prefix(prefix.as_bytes()) {
        let (_key, value) = guard.into_inner().map_err(|e| {
            error::StoreSnafu {
                message: format!("ci_validation prefix scan: {e}"),
            }
            .build()
        })?;
        results.push(deserialize_value::<CiValidationRecord>(&value)?);
    }
    Ok(results)
}
