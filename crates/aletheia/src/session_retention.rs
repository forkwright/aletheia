//! Retention executor for session-scoped cleanup.

use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use mneme::store::SessionStore;
use mneme::types::{AgentNote, Message, Session, SessionStatus, UsageRecord};
use oikonomos::maintenance::{RetentionExecutor, RetentionSummary};
use serde::Serialize;
use taxis::config::RetentionSettings;
use taxis::oikos::Oikos;
use tokio::sync::Mutex;
use tracing::{info, warn};

/// Bridges the daemon retention task to the fjall-backed session store.
pub(crate) struct SessionRetentionAdapter {
    store: Arc<Mutex<SessionStore>>,
    settings: Option<RetentionSettings>,
}

impl SessionRetentionAdapter {
    pub(crate) fn new(store: Arc<Mutex<SessionStore>>) -> Self {
        Self {
            store,
            settings: None,
        }
    }

    #[cfg(test)]
    fn new_with_settings(store: Arc<Mutex<SessionStore>>, settings: RetentionSettings) -> Self {
        Self {
            store,
            settings: Some(settings),
        }
    }

    fn resolve_settings(
        &self,
        store: &SessionStore,
    ) -> oikonomos::error::Result<RetentionSettings> {
        if let Some(settings) = &self.settings {
            return Ok(settings.clone());
        }

        let data_dir = store.path().parent().ok_or_else(|| {
            retention_failure(format!(
                "session store path has no parent: {}",
                store.path().display()
            ))
        })?;
        let instance_root = data_dir.parent().ok_or_else(|| {
            retention_failure(format!(
                "session store data dir has no parent: {}",
                data_dir.display()
            ))
        })?;
        let oikos = Oikos::from_root(instance_root);
        let config = taxis::loader::load_config(&oikos).map_err(|e| {
            retention_failure(format!(
                "load retention config from {} failed: {e}",
                oikos.config().join("aletheia.toml").display()
            ))
        })?;
        Ok(config.maintenance.retention)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionArchive<'a> {
    version: u32,
    archived_at: String,
    session: &'a Session,
    messages: Vec<Message>,
    usage_records: Vec<UsageRecord>,
    notes: Vec<AgentNote>,
}

struct ArchiveStats {
    path: PathBuf,
    message_count: u32,
    bytes_written: u64,
}

#[derive(Default)]
struct RetentionCounters {
    sessions_cleaned: u32,
    cap_sessions_cleaned: u32,
    messages_cleaned: u32,
    bytes_freed: u64,
}

impl RetentionCounters {
    fn add(&mut self, other: &Self) {
        self.sessions_cleaned = self.sessions_cleaned.saturating_add(other.sessions_cleaned);
        self.cap_sessions_cleaned = self
            .cap_sessions_cleaned
            .saturating_add(other.cap_sessions_cleaned);
        self.messages_cleaned = self.messages_cleaned.saturating_add(other.messages_cleaned);
        self.bytes_freed = self.bytes_freed.saturating_add(other.bytes_freed);
    }
}

impl RetentionExecutor for SessionRetentionAdapter {
    fn execute_retention(&self) -> oikonomos::error::Result<RetentionSummary> {
        let store = self.store.blocking_lock();
        let settings = self.resolve_settings(&store)?;
        let archive_dir = archive_dir_for_store(&store)?;

        let blackboard_entries_cleaned = cleanup_blackboard_entries(&store)?;
        let mut counters = RetentionCounters::default();

        if settings.enabled {
            counters.messages_cleaned = counters
                .messages_cleaned
                .saturating_add(cleanup_orphan_messages(&store, &settings)?);
            cleanup_usage_records(&store)?;
            counters.add(&cleanup_closed_sessions(&store, &settings, &archive_dir)?);
            counters.add(&enforce_session_cap(&store, &settings, &archive_dir)?);
        }

        let (archive_files_pruned, archive_bytes_freed) =
            prune_session_archive_dir(&archive_dir, settings.archive_ttl_days)?;
        counters.bytes_freed = counters.bytes_freed.saturating_add(archive_bytes_freed);

        if archive_files_pruned > 0 {
            info!(
                archive_files_pruned = archive_files_pruned,
                archive_bytes_freed = archive_bytes_freed,
                archive_ttl_days = ?settings.archive_ttl_days,
                "session archive pruning completed"
            );
        }

        Ok(RetentionSummary {
            sessions_cleaned: counters.sessions_cleaned,
            messages_cleaned: counters.messages_cleaned,
            blackboard_entries_cleaned,
            cap_sessions_cleaned: counters.cap_sessions_cleaned,
            bytes_freed: counters.bytes_freed,
        })
    }
}

fn retention_failure(reason: impl Into<String>) -> oikonomos::error::Error {
    oikonomos::error::TaskFailedSnafu {
        task_id: "retention-execution",
        reason: reason.into(),
    }
    .build()
}

fn cleanup_blackboard_entries(store: &SessionStore) -> oikonomos::error::Result<u32> {
    let cleaned = store
        .cleanup_expired_entries()
        .map_err(|e| retention_failure(format!("blackboard cleanup failed: {e}")))?;
    u32::try_from(cleaned)
        .map_err(|e| retention_failure(format!("blackboard cleanup count overflow: {e}")))
}

fn cleanup_orphan_messages(
    store: &SessionStore,
    settings: &RetentionSettings,
) -> oikonomos::error::Result<u32> {
    let Some(ttl_days) = settings.orphan_message_max_age_days else {
        return Ok(0);
    };
    let cutoff = cutoff_iso(ttl_days);
    let cleaned = store
        .cleanup_orphan_messages(&cutoff)
        .map_err(|e| retention_failure(format!("orphan message cleanup failed: {e}")))?;
    u32::try_from(cleaned)
        .map_err(|e| retention_failure(format!("orphan message cleanup count overflow: {e}")))
}

/// Prune per-session usage records so the usage partition cannot grow without
/// bound (#5660).
///
/// WHY: `RetentionSettings` has no usage-specific knob today, so the cap is a
/// fixed `USAGE_RECORDS_KEEP_LAST` — large enough that no live session loses
/// recent accounting, small enough to bound long-lived sessions.
fn cleanup_usage_records(store: &SessionStore) -> oikonomos::error::Result<()> {
    /// WHY: keep the most recent N usage rows per session; bounds growth without
    /// a config knob (#5660).
    const USAGE_RECORDS_KEEP_LAST: u64 = 5000;

    let all_sessions = store
        .list_sessions(None)
        .map_err(|e| retention_failure(format!("list sessions for usage cleanup failed: {e}")))?;
    for session in all_sessions {
        store
            .cleanup_usage_records(&session.id, USAGE_RECORDS_KEEP_LAST)
            .map_err(|e| {
                retention_failure(format!(
                    "usage cleanup for session '{}' failed: {e}",
                    session.id
                ))
            })?;
    }
    Ok(())
}

fn cleanup_closed_sessions(
    store: &SessionStore,
    settings: &RetentionSettings,
    archive_dir: &Path,
) -> oikonomos::error::Result<RetentionCounters> {
    let Some(ttl_days) = settings.closed_session_ttl_days else {
        return Ok(RetentionCounters::default());
    };
    let cutoff = cutoff_iso(ttl_days);
    let all_sessions = store
        .list_sessions(None)
        .map_err(|e| retention_failure(format!("list sessions failed: {e}")))?;
    let mut counters = RetentionCounters::default();

    for session in all_sessions {
        // WHY: lexicographic comparison is correct for fixed-format ISO 8601 UTC
        // timestamps (YYYY-MM-DDTHH:MM:SSZ).
        if session.updated_at.as_str() >= cutoff.as_str() {
            continue;
        }

        match session.status {
            SessionStatus::Archived | SessionStatus::Distilled => {
                let archive_stats = if settings.archive_before_delete {
                    Some(write_session_archive(store, archive_dir, &session)?)
                } else {
                    None
                };
                store.delete_session(&session.id).map_err(|e| {
                    retention_failure(format!("delete session '{}' failed: {e}", session.id))
                })?;
                counters.sessions_cleaned = counters.sessions_cleaned.saturating_add(1);
                record_session_cleanup(&mut counters, &session, archive_stats);
            }
            // SessionStatus is non_exhaustive; skip unknown future variants.
            _ => {}
        }
    }

    if counters.sessions_cleaned > 0 {
        info!(
            sessions_cleaned = counters.sessions_cleaned,
            messages_cleaned = counters.messages_cleaned,
            bytes_freed = counters.bytes_freed,
            ttl_days,
            "session retention pass completed"
        );
    }
    Ok(counters)
}

fn enforce_session_cap(
    store: &SessionStore,
    settings: &RetentionSettings,
    archive_dir: &Path,
) -> oikonomos::error::Result<RetentionCounters> {
    if settings.max_sessions_per_nous == 0 {
        return Ok(RetentionCounters::default());
    }
    let all_sessions = store
        .list_sessions(None)
        .map_err(|e| retention_failure(format!("list sessions failed: {e}")))?;

    let mut counters = RetentionCounters::default();

    let mut by_nous: std::collections::HashMap<&str, Vec<&Session>> =
        std::collections::HashMap::new();
    for session in &all_sessions {
        by_nous.entry(&session.nous_id).or_default().push(session);
    }

    let cap = usize::try_from(settings.max_sessions_per_nous)
        .map_err(|e| retention_failure(format!("session cap conversion failed: {e}")))?;
    for sessions in by_nous.values_mut() {
        sessions.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| a.id.cmp(&b.id))
        });

        for session in sessions.iter().skip(cap) {
            match session.status {
                SessionStatus::Archived | SessionStatus::Distilled => {
                    let archive_stats = if settings.archive_before_delete {
                        Some(write_session_archive(store, archive_dir, session)?)
                    } else {
                        None
                    };
                    store.delete_session(&session.id).map_err(|e| {
                        retention_failure(format!("delete session {} failed: {e}", session.id))
                    })?;
                    counters.sessions_cleaned = counters.sessions_cleaned.saturating_add(1);
                    counters.cap_sessions_cleaned = counters.cap_sessions_cleaned.saturating_add(1);
                    record_session_cleanup(&mut counters, session, archive_stats);
                }
                _ => {}
            }
        }
    }

    if counters.cap_sessions_cleaned > 0 {
        info!(
            cap_sessions_cleaned = counters.cap_sessions_cleaned,
            sessions_cleaned = counters.sessions_cleaned,
            messages_cleaned = counters.messages_cleaned,
            bytes_freed = counters.bytes_freed,
            max_sessions_per_nous = settings.max_sessions_per_nous,
            "session cap retention pass completed"
        );
    }

    Ok(counters)
}

fn record_session_cleanup(
    counters: &mut RetentionCounters,
    session: &Session,
    archive_stats: Option<ArchiveStats>,
) {
    if let Some(stats) = archive_stats {
        counters.messages_cleaned = counters
            .messages_cleaned
            .saturating_add(stats.message_count);
        counters.bytes_freed = counters.bytes_freed.saturating_add(stats.bytes_written);
        info!(
            session_id = %session.id,
            archive_path = %stats.path.display(),
            messages = stats.message_count,
            bytes = stats.bytes_written,
            "session retention archived deleted session"
        );
    } else {
        counters.messages_cleaned = counters
            .messages_cleaned
            .saturating_add(message_count_to_u32(session.metrics.message_count));
    }
}

fn archive_dir_for_store(store: &SessionStore) -> oikonomos::error::Result<PathBuf> {
    let data_dir = store.path().parent().ok_or_else(|| {
        retention_failure(format!(
            "session store path has no parent: {}",
            store.path().display()
        ))
    })?;
    Ok(data_dir.join("archive").join("sessions"))
}

/// Prune session JSON archives older than `archive_ttl_days`.
///
/// WHY: `archive_before_delete` writes one JSON file per deleted session. Without
/// a TTL the archive directory grows without bound and can exhaust disk (#5658).
fn prune_session_archive_dir(
    archive_dir: &Path,
    archive_ttl_days: Option<u32>,
) -> oikonomos::error::Result<(u32, u64)> {
    let Some(ttl_days) = archive_ttl_days else {
        return Ok((0, 0));
    };

    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(u64::from(ttl_days) * 86400))
        .ok_or_else(|| retention_failure("archive TTL cutoff overflow"))?;

    let mut files_pruned: u32 = 0;
    let mut bytes_freed: u64 = 0;

    let read_dir = match fs::read_dir(archive_dir) {
        Ok(dir) => dir,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((0, 0)),
        Err(e) => {
            return Err(retention_failure(format!(
                "read archive dir {} failed: {e}",
                archive_dir.display()
            )));
        }
    };

    for entry in read_dir {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                warn!(error = %e, "skipping unreadable archive directory entry");
                continue;
            }
        };

        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        if !path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
        {
            continue;
        }

        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(e) => {
                warn!(path = %path.display(), error = %e, "unable to read archive file metadata");
                continue;
            }
        };

        let modified = match metadata.modified() {
            Ok(time) => time,
            Err(e) => {
                warn!(path = %path.display(), error = %e, "unable to read archive file mtime");
                continue;
            }
        };

        if modified >= cutoff {
            continue;
        }

        let file_size = metadata.len();

        if let Err(e) = fs::remove_file(&path) {
            warn!(
                path = %path.display(),
                error = %e,
                "failed to prune stale session archive"
            );
            continue;
        }

        files_pruned = files_pruned.saturating_add(1);
        bytes_freed = bytes_freed.saturating_add(file_size);
    }

    Ok((files_pruned, bytes_freed))
}

fn write_session_archive(
    store: &SessionStore,
    archive_dir: &Path,
    session: &Session,
) -> oikonomos::error::Result<ArchiveStats> {
    let messages = store.get_history_raw(&session.id, None).map_err(|e| {
        retention_failure(format!(
            "read messages for archive session '{}' failed: {e}",
            session.id
        ))
    })?;
    let usage_records = store.get_usage_for_session(&session.id).map_err(|e| {
        retention_failure(format!(
            "read usage for archive session '{}' failed: {e}",
            session.id
        ))
    })?;
    let notes = store.get_notes(&session.id).map_err(|e| {
        retention_failure(format!(
            "read notes for archive session '{}' failed: {e}",
            session.id
        ))
    })?;

    let message_count = u32::try_from(messages.len()).unwrap_or(u32::MAX);
    let archive = SessionArchive {
        version: 1,
        archived_at: jiff::Timestamp::now().to_string(),
        session,
        messages,
        usage_records,
        notes,
    };
    let bytes = serde_json::to_vec_pretty(&archive).map_err(|e| {
        retention_failure(format!(
            "serialize archive for session '{}' failed: {e}",
            session.id
        ))
    })?;
    let bytes_written = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    let path = archive_dir.join(format!("{}.json", archive_file_stem(&session.id)));
    write_archive_file(&path, &bytes).map_err(|e| {
        retention_failure(format!(
            "write archive for session '{}' to {} failed: {e}",
            session.id,
            path.display()
        ))
    })?;

    Ok(ArchiveStats {
        path,
        message_count,
        bytes_written,
    })
}

fn archive_file_stem(session_id: &str) -> String {
    let mut stem = String::with_capacity(session_id.len());
    for ch in session_id.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            stem.push(ch);
        } else {
            stem.push('_');
        }
    }
    if stem.is_empty() {
        "session".to_owned()
    } else {
        stem
    }
}

fn message_count_to_u32(count: i64) -> u32 {
    u32::try_from(count.max(0)).unwrap_or(u32::MAX)
}

fn write_archive_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("archive path has no parent"))?;
    fs::create_dir_all(parent)?;

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("session.json");
    let tmp_path = parent.join(format!(".{file_name}.tmp"));

    {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp_path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
    }

    fs::rename(&tmp_path, path)?;
    #[expect(
        clippy::disallowed_methods,
        reason = "archive writes need synchronous parent-directory fsync after rename for crash durability"
    )]
    let dir = File::open(parent)?;
    dir.sync_all()?;
    Ok(())
}

/// Compute the ISO 8601 UTC cutoff timestamp for `ttl_days` days ago.
///
/// Sessions with `updated_at` strictly before this value are eligible for
/// retention processing.
fn cutoff_iso(ttl_days: u32) -> String {
    // WHY: jiff is the project-standard time library (see CLAUDE.md key patterns).
    // We compute now minus ttl_days as a Zoned timestamp and format in the same
    // fixed ISO 8601 format the store uses for updated_at.
    use jiff::{Timestamp, ToSpan as _};
    let days: i64 = i64::from(ttl_days);
    let cutoff = Timestamp::now()
        .checked_sub(days.days())
        .unwrap_or(Timestamp::UNIX_EPOCH);
    cutoff.strftime("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions index a known-shape JSON archive"
)]
mod tests {
    use mneme::types::{SessionMetrics, SessionOrigin, SessionType};
    use oikonomos::maintenance::RetentionExecutor as _;

    use super::*;

    fn retention_error(reason: impl Into<String>) -> oikonomos::error::Error {
        oikonomos::error::TaskFailedSnafu {
            task_id: "retention-execution",
            reason: reason.into(),
        }
        .build()
    }

    fn session_fixture(
        id: &str,
        nous_id: &str,
        session_key: &str,
        status: SessionStatus,
        updated_at: &str,
    ) -> Session {
        Session {
            id: id.to_owned(),
            nous_id: nous_id.to_owned(),
            session_key: session_key.to_owned(),
            status,
            model: None,
            session_type: SessionType::Primary,
            created_at: updated_at.to_owned(),
            updated_at: updated_at.to_owned(),
            metrics: SessionMetrics {
                token_count_estimate: 0,
                message_count: 0,
                last_input_tokens: 0,
                bootstrap_hash: None,
                distillation_count: 0,
                last_distilled_at: None,
                computed_context_tokens: 0,
            },
            origin: SessionOrigin {
                parent_session_id: None,
                thread_id: None,
                transport: None,
                display_name: None,
            },
            artefact_meta: None,
        }
    }

    fn import_fixture(
        store: &SessionStore,
        id: &str,
        nous_id: &str,
        status: SessionStatus,
        updated_at: &str,
    ) -> oikonomos::error::Result<()> {
        store
            .import_session(
                &session_fixture(id, nous_id, &format!("key-{id}"), status, updated_at),
                false,
            )
            .map_err(|e| retention_error(format!("import {id}: {e}")))?;
        Ok(())
    }

    fn session_exists(store: &SessionStore, id: &str) -> oikonomos::error::Result<bool> {
        store
            .find_session_by_id(id)
            .map(|session| session.is_some())
            .map_err(|e| retention_error(format!("find {id}: {e}")))
    }

    #[tokio::test]
    async fn retention_adapter_executes_blackboard_cleanup() -> oikonomos::error::Result<()> {
        let store =
            Arc::new(Mutex::new(SessionStore::open_in_memory().map_err(|e| {
                retention_error(format!("session store open failed: {e}"))
            })?));

        let adapter = SessionRetentionAdapter::new_with_settings(
            Arc::clone(&store),
            RetentionSettings::default(),
        );
        let summary = tokio::task::spawn_blocking(move || adapter.execute_retention())
            .await
            .map_err(|e| retention_error(format!("retention task join failed: {e}")))??;

        assert_eq!(summary.blackboard_entries_cleaned, 0);
        assert_eq!(summary.sessions_cleaned, 0);
        let entries = store
            .lock()
            .await
            .blackboard_list()
            .map_err(|e| retention_error(format!("blackboard list failed: {e}")))?;
        assert!(entries.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn retention_disabled_skips_session_cleanup() -> oikonomos::error::Result<()> {
        let store = Arc::new(Mutex::new(
            SessionStore::open_in_memory()
                .map_err(|e| retention_error(format!("store open: {e}")))?,
        ));
        {
            let locked = store.lock().await;
            locked
                .create_session("ses-old", "syn", "main", None, None)
                .map_err(|e| retention_error(format!("create: {e}")))?;
        }

        let settings = RetentionSettings {
            enabled: false,
            closed_session_ttl_days: Some(0),
            archive_before_delete: true,
            ..RetentionSettings::default()
        };
        let adapter = SessionRetentionAdapter::new_with_settings(Arc::clone(&store), settings);
        let summary = tokio::task::spawn_blocking(move || adapter.execute_retention())
            .await
            .map_err(|e| retention_error(format!("join: {e}")))??;

        assert_eq!(
            summary.sessions_cleaned, 0,
            "disabled retention must not clean sessions"
        );
        Ok(())
    }

    #[tokio::test]
    async fn retention_no_ttl_skips_session_cleanup() -> oikonomos::error::Result<()> {
        let store = Arc::new(Mutex::new(
            SessionStore::open_in_memory()
                .map_err(|e| retention_error(format!("store open: {e}")))?,
        ));
        {
            let locked = store.lock().await;
            locked
                .create_session("ses-old", "syn", "main", None, None)
                .map_err(|e| retention_error(format!("create: {e}")))?;
        }

        let settings = RetentionSettings {
            enabled: true,
            closed_session_ttl_days: None,
            archive_before_delete: true,
            ..RetentionSettings::default()
        };
        let adapter = SessionRetentionAdapter::new_with_settings(Arc::clone(&store), settings);
        let summary = tokio::task::spawn_blocking(move || adapter.execute_retention())
            .await
            .map_err(|e| retention_error(format!("join: {e}")))??;

        assert_eq!(
            summary.sessions_cleaned, 0,
            "no ttl means no session cleanup"
        );
        Ok(())
    }

    #[tokio::test]
    async fn retention_skips_active_sessions() -> oikonomos::error::Result<()> {
        let store = Arc::new(Mutex::new(
            SessionStore::open_in_memory()
                .map_err(|e| retention_error(format!("store open: {e}")))?,
        ));
        {
            let locked = store.lock().await;
            locked
                .create_session("ses-active", "syn", "key-a", None, None)
                .map_err(|e| retention_error(format!("create: {e}")))?;
        }

        let settings = RetentionSettings {
            enabled: true,
            closed_session_ttl_days: Some(0),
            archive_before_delete: true,
            ..RetentionSettings::default()
        };
        let adapter = SessionRetentionAdapter::new_with_settings(Arc::clone(&store), settings);
        let summary = tokio::task::spawn_blocking(move || adapter.execute_retention())
            .await
            .map_err(|e| retention_error(format!("join: {e}")))??;

        assert_eq!(
            summary.sessions_cleaned, 0,
            "active session must not be deleted by closed-session retention"
        );
        let session = store
            .lock()
            .await
            .find_session_by_id("ses-active")
            .map_err(|e| retention_error(format!("find: {e}")))?;
        assert_eq!(
            session.map(|s| s.status),
            Some(SessionStatus::Active),
            "active session must remain active after retention"
        );
        Ok(())
    }

    #[tokio::test]
    async fn retention_exports_archived_session_before_delete() -> oikonomos::error::Result<()> {
        let store = Arc::new(Mutex::new(
            SessionStore::open_in_memory()
                .map_err(|e| retention_error(format!("store open: {e}")))?,
        ));
        {
            let locked = store.lock().await;
            locked
                .create_session("ses-arc", "syn", "key-b", None, None)
                .map_err(|e| retention_error(format!("create: {e}")))?;
            locked
                .append_message(
                    "ses-arc",
                    mneme::types::Role::User,
                    "archive me",
                    None,
                    None,
                    2,
                )
                .map_err(|e| retention_error(format!("append: {e}")))?;
            locked
                .update_session_status("ses-arc", SessionStatus::Archived)
                .map_err(|e| retention_error(format!("archive: {e}")))?;
        }

        let settings = RetentionSettings {
            enabled: true,
            closed_session_ttl_days: Some(0),
            archive_before_delete: true,
            ..RetentionSettings::default()
        };
        let adapter = SessionRetentionAdapter::new_with_settings(Arc::clone(&store), settings);
        let summary = tokio::task::spawn_blocking(move || adapter.execute_retention())
            .await
            .map_err(|e| retention_error(format!("join: {e}")))??;

        assert_eq!(summary.sessions_cleaned, 1);
        assert_eq!(summary.messages_cleaned, 1);
        assert!(
            summary.bytes_freed > 0,
            "archive byte count should be reported"
        );
        let locked = store.lock().await;
        let archive_path = archive_dir_for_store(&locked)?.join("ses-arc.json");
        let archive = std::fs::read_to_string(&archive_path)
            .map_err(|e| retention_error(format!("read archive: {e}")))?;
        let archive_json: serde_json::Value = serde_json::from_str(&archive)
            .map_err(|e| retention_error(format!("parse archive: {e}")))?;
        assert_eq!(archive_json["session"]["id"], "ses-arc");
        assert_eq!(archive_json["messages"][0]["content"], "archive me");
        let session = locked
            .find_session_by_id("ses-arc")
            .map_err(|e| retention_error(format!("find: {e}")))?;
        assert!(
            session.is_none(),
            "archived session must be deleted after archive write"
        );
        Ok(())
    }

    #[tokio::test]
    async fn retention_cap_zero_is_unlimited() -> oikonomos::error::Result<()> {
        let store = Arc::new(Mutex::new(
            SessionStore::open_in_memory()
                .map_err(|e| retention_error(format!("store open: {e}")))?,
        ));
        {
            let locked = store.lock().await;
            import_fixture(
                &locked,
                "ses-a",
                "syn",
                SessionStatus::Archived,
                "2024-01-01T00:00:00Z",
            )?;
            import_fixture(
                &locked,
                "ses-b",
                "syn",
                SessionStatus::Distilled,
                "2024-01-02T00:00:00Z",
            )?;
            import_fixture(
                &locked,
                "ses-c",
                "syn",
                SessionStatus::Archived,
                "2024-01-03T00:00:00Z",
            )?;
        }

        let settings = RetentionSettings {
            enabled: true,
            closed_session_ttl_days: None,
            max_sessions_per_nous: 0,
            archive_before_delete: true,
            ..RetentionSettings::default()
        };
        let adapter = SessionRetentionAdapter::new_with_settings(Arc::clone(&store), settings);
        let summary = tokio::task::spawn_blocking(move || adapter.execute_retention())
            .await
            .map_err(|e| retention_error(format!("join: {e}")))??;

        assert_eq!(summary.sessions_cleaned, 0);
        assert_eq!(summary.cap_sessions_cleaned, 0);
        let locked = store.lock().await;
        for id in ["ses-a", "ses-b", "ses-c"] {
            assert!(session_exists(&locked, id)?);
        }
        Ok(())
    }

    #[tokio::test]
    async fn retention_cap_enforces_per_nous_and_preserves_active_sessions()
    -> oikonomos::error::Result<()> {
        let store = Arc::new(Mutex::new(
            SessionStore::open_in_memory()
                .map_err(|e| retention_error(format!("store open: {e}")))?,
        ));
        {
            let locked = store.lock().await;
            import_fixture(
                &locked,
                "syn-active-new",
                "syn",
                SessionStatus::Active,
                "2024-01-05T00:00:00Z",
            )?;
            import_fixture(
                &locked,
                "syn-arch-keep",
                "syn",
                SessionStatus::Archived,
                "2024-01-04T00:00:00Z",
            )?;
            import_fixture(
                &locked,
                "syn-dist-drop",
                "syn",
                SessionStatus::Distilled,
                "2024-01-03T00:00:00Z",
            )?;
            import_fixture(
                &locked,
                "syn-arch-drop",
                "syn",
                SessionStatus::Archived,
                "2024-01-02T00:00:00Z",
            )?;
            import_fixture(
                &locked,
                "syn-active-old",
                "syn",
                SessionStatus::Active,
                "2024-01-01T00:00:00Z",
            )?;
            import_fixture(
                &locked,
                "bob-arch-keep",
                "bob",
                SessionStatus::Archived,
                "2024-01-03T00:00:00Z",
            )?;
            import_fixture(
                &locked,
                "bob-dist-keep",
                "bob",
                SessionStatus::Distilled,
                "2024-01-02T00:00:00Z",
            )?;
            import_fixture(
                &locked,
                "bob-arch-drop",
                "bob",
                SessionStatus::Archived,
                "2024-01-01T00:00:00Z",
            )?;
        }

        let settings = RetentionSettings {
            enabled: true,
            closed_session_ttl_days: None,
            max_sessions_per_nous: 2,
            archive_before_delete: false,
            ..RetentionSettings::default()
        };
        let adapter = SessionRetentionAdapter::new_with_settings(Arc::clone(&store), settings);
        let summary = tokio::task::spawn_blocking(move || adapter.execute_retention())
            .await
            .map_err(|e| retention_error(format!("join: {e}")))??;

        assert_eq!(summary.sessions_cleaned, 3);
        assert_eq!(summary.cap_sessions_cleaned, 3);
        let locked = store.lock().await;
        for id in [
            "syn-active-new",
            "syn-arch-keep",
            "syn-active-old",
            "bob-arch-keep",
            "bob-dist-keep",
        ] {
            assert!(session_exists(&locked, id)?, "{id} should remain");
        }
        for id in ["syn-dist-drop", "syn-arch-drop", "bob-arch-drop"] {
            assert!(!session_exists(&locked, id)?, "{id} should be deleted");
        }
        Ok(())
    }

    #[tokio::test]
    async fn retention_cap_uses_session_id_for_stable_ties() -> oikonomos::error::Result<()> {
        let store = Arc::new(Mutex::new(
            SessionStore::open_in_memory()
                .map_err(|e| retention_error(format!("store open: {e}")))?,
        ));
        {
            let locked = store.lock().await;
            import_fixture(
                &locked,
                "b",
                "syn",
                SessionStatus::Archived,
                "2024-01-01T00:00:00Z",
            )?;
            import_fixture(
                &locked,
                "a",
                "syn",
                SessionStatus::Archived,
                "2024-01-01T00:00:00Z",
            )?;
        }

        let settings = RetentionSettings {
            enabled: true,
            closed_session_ttl_days: None,
            max_sessions_per_nous: 1,
            archive_before_delete: false,
            ..RetentionSettings::default()
        };
        let adapter = SessionRetentionAdapter::new_with_settings(Arc::clone(&store), settings);
        let summary = tokio::task::spawn_blocking(move || adapter.execute_retention())
            .await
            .map_err(|e| retention_error(format!("join: {e}")))??;

        assert_eq!(summary.sessions_cleaned, 1);
        assert_eq!(summary.cap_sessions_cleaned, 1);
        let locked = store.lock().await;
        assert!(session_exists(&locked, "a")?);
        assert!(!session_exists(&locked, "b")?);
        Ok(())
    }

    #[tokio::test]
    async fn retention_cap_archives_before_delete() -> oikonomos::error::Result<()> {
        let store = Arc::new(Mutex::new(
            SessionStore::open_in_memory()
                .map_err(|e| retention_error(format!("store open: {e}")))?,
        ));
        {
            let locked = store.lock().await;
            locked
                .create_session("cap-del", "syn", "key-cap-del", None, None)
                .map_err(|e| retention_error(format!("create: {e}")))?;
            locked
                .append_message(
                    "cap-del",
                    mneme::types::Role::User,
                    "cap archive me",
                    None,
                    None,
                    2,
                )
                .map_err(|e| retention_error(format!("append: {e}")))?;
            locked
                .import_session(
                    &session_fixture(
                        "cap-del",
                        "syn",
                        "key-cap-del",
                        SessionStatus::Archived,
                        "2024-01-01T00:00:00Z",
                    ),
                    true,
                )
                .map_err(|e| retention_error(format!("import cap-del: {e}")))?;
            import_fixture(
                &locked,
                "keep",
                "syn",
                SessionStatus::Archived,
                "2024-01-02T00:00:00Z",
            )?;
        }

        let settings = RetentionSettings {
            enabled: true,
            closed_session_ttl_days: None,
            max_sessions_per_nous: 1,
            archive_before_delete: true,
            ..RetentionSettings::default()
        };
        let adapter = SessionRetentionAdapter::new_with_settings(Arc::clone(&store), settings);
        let summary = tokio::task::spawn_blocking(move || adapter.execute_retention())
            .await
            .map_err(|e| retention_error(format!("join: {e}")))??;

        assert_eq!(summary.sessions_cleaned, 1);
        assert_eq!(summary.cap_sessions_cleaned, 1);
        assert_eq!(summary.messages_cleaned, 1);
        assert!(summary.bytes_freed > 0);
        let locked = store.lock().await;
        let archive_path = archive_dir_for_store(&locked)?.join("cap-del.json");
        let archive = std::fs::read_to_string(&archive_path)
            .map_err(|e| retention_error(format!("read archive: {e}")))?;
        let archive_json: serde_json::Value = serde_json::from_str(&archive)
            .map_err(|e| retention_error(format!("parse archive: {e}")))?;
        assert_eq!(archive_json["session"]["id"], "cap-del");
        assert_eq!(archive_json["messages"][0]["content"], "cap archive me");
        assert!(!session_exists(&locked, "cap-del")?);
        assert!(session_exists(&locked, "keep")?);
        Ok(())
    }

    #[tokio::test]
    async fn retention_prunes_session_archives_older_than_ttl() -> oikonomos::error::Result<()> {
        let store = Arc::new(Mutex::new(
            SessionStore::open_in_memory()
                .map_err(|e| retention_error(format!("store open: {e}")))?,
        ));

        let archive_dir = {
            let locked = store.lock().await;
            let dir = archive_dir_for_store(&locked)?;
            fs::create_dir_all(&dir)
                .map_err(|e| retention_error(format!("create archive dir: {e}")))?;
            dir
        };

        let stale_path = archive_dir.join("stale.json");
        let recent_path = archive_dir.join("recent.json");
        write_archive_file(&stale_path, b"old")
            .map_err(|e| retention_error(format!("write stale archive: {e}")))?;
        write_archive_file(&recent_path, b"recent")
            .map_err(|e| retention_error(format!("write recent archive: {e}")))?;

        let stale_time = SystemTime::now()
            .checked_sub(Duration::from_hours(120))
            .ok_or_else(|| retention_error("stale archive time overflow"))?;
        let file = OpenOptions::new()
            .write(true)
            .open(&stale_path)
            .map_err(|e| retention_error(format!("open stale archive: {e}")))?;
        file.set_times(std::fs::FileTimes::new().set_modified(stale_time))
            .map_err(|e| retention_error(format!("set stale archive mtime: {e}")))?;

        let settings = RetentionSettings {
            enabled: false,
            archive_ttl_days: Some(2),
            ..RetentionSettings::default()
        };
        let adapter = SessionRetentionAdapter::new_with_settings(Arc::clone(&store), settings);
        let summary = tokio::task::spawn_blocking(move || adapter.execute_retention())
            .await
            .map_err(|e| retention_error(format!("join: {e}")))??;

        assert!(
            !stale_path.exists(),
            "archive file older than TTL should be pruned"
        );
        assert!(
            recent_path.exists(),
            "archive file newer than TTL should remain"
        );
        assert!(
            summary.bytes_freed >= 3,
            "pruned archive bytes should be counted in summary"
        );
        Ok(())
    }
}
