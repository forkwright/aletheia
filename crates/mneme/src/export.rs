//! Agent export: build an `AgentFile` from session store and workspace.
#![cfg_attr(
    any(feature = "mneme-engine", test),
    expect(
        clippy::indexing_slicing,
        reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
    )
)]

use std::collections::HashMap;
use std::path::Path;

use snafu::ResultExt;
use tracing::{info, instrument, warn};

use crate::error::{self, Result};
use crate::portability::{
    AGENT_FILE_VERSION, AgentFile, ExportedMessage, ExportedNote, ExportedSession, NousInfo,
    WorkspaceData,
};
use crate::store::SessionStore;
use crate::types::SessionStatus;

/// Maximum file size to include in export (10 MB).
const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;

/// Byte scan window for binary detection.
const BINARY_PROBE_SIZE: usize = 8192;

/// Directories to skip during workspace scan.
const IGNORE_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "__pycache__",
    ".cache",
    "dist",
];

/// Options controlling what gets exported.
#[derive(Debug, Clone)]
pub struct ExportOptions {
    /// Maximum messages per session (0 = all).
    pub max_messages_per_session: usize,
    /// Include archived/distilled sessions.
    pub include_archived: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            max_messages_per_session: 500,
            include_archived: false,
        }
    }
}

/// Export an agent to a portable `AgentFile`.
///
/// The caller resolves paths and config from Oikos/taxis, then passes
/// simple types here: mneme never touches taxis.
///
/// # Errors
///
/// Returns an error if session store queries or workspace I/O fails.
#[instrument(skip(store))]
pub fn export_agent(
    nous_id: &str,
    agent_name: Option<&str>,
    agent_model: Option<&str>,
    agent_config: serde_json::Value,
    store: &SessionStore,
    workspace_path: &Path,
    opts: &ExportOptions,
) -> Result<AgentFile> {
    let workspace = scan_workspace(workspace_path)?;

    let all_sessions = store.list_sessions(Some(nous_id))?;
    let filtered: Vec<_> = if opts.include_archived {
        all_sessions
    } else {
        all_sessions
            .into_iter()
            .filter(|s| s.status == SessionStatus::Active)
            .collect()
    };

    let mut sessions = Vec::with_capacity(filtered.len());
    for session in &filtered {
        sessions.push(export_session(store, session, opts)?);
    }

    let exported_at = jiff::Timestamp::now()
        .strftime("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let generator = format!("aletheia-rust/{}", env!("CARGO_PKG_VERSION"));

    info!(
        nous_id,
        sessions = sessions.len(),
        workspace_files = workspace.files.len(),
        binary_files = workspace.binary_files.len(),
        "agent exported"
    );

    Ok(AgentFile {
        version: AGENT_FILE_VERSION,
        exported_at,
        generator,
        nous: NousInfo {
            id: nous_id.to_owned(),
            name: agent_name.map(String::from),
            model: agent_model.map(String::from),
            config: agent_config,
        },
        workspace,
        sessions,
        memory: None,
        knowledge: None,
    })
}

/// Build a `KnowledgeExport` from the knowledge store.
///
/// Queries all facts, entities, and relationships for the given nous.
/// Returns `None` if the store is empty or the query fails.
#[cfg(feature = "mneme-engine")]
#[instrument(skip(store))]
pub fn export_knowledge(
    nous_id: &str,
    store: &crate::knowledge_store::KnowledgeStore,
) -> Option<crate::portability::KnowledgeExport> {
    let facts = store
        .query_facts(nous_id, "9999-01-01T00:00:00Z", 100_000)
        .ok()
        .unwrap_or_default();

    let entities = query_all_entities(store).unwrap_or_default();

    let relationships = query_all_relationships(store).unwrap_or_default();

    if facts.is_empty() && entities.is_empty() && relationships.is_empty() {
        return None;
    }

    info!(
        nous_id,
        facts = facts.len(),
        entities = entities.len(),
        relationships = relationships.len(),
        "knowledge exported"
    );

    Some(crate::portability::KnowledgeExport {
        facts,
        entities,
        relationships,
    })
}

/// Query all entities from the knowledge store.
#[cfg(feature = "mneme-engine")]
fn query_all_entities(
    store: &crate::knowledge_store::KnowledgeStore,
) -> Result<Vec<crate::knowledge::Entity>> {
    use std::collections::BTreeMap;

    let script = r"?[id, name, entity_type, aliases, created_at, updated_at] := *entities{id, name, entity_type, aliases, created_at, updated_at}";
    let rows = store.run_query(script, BTreeMap::new())?;

    let mut entities = Vec::new();
    for row in &rows.rows {
        if row.len() < 6 {
            continue;
        }
        let id = crate::id::EntityId::new_unchecked(row[0].get_str().unwrap_or_default());
        let name = row[1].get_str().unwrap_or_default().to_owned();
        let entity_type = row[2].get_str().unwrap_or_default().to_owned();
        let aliases_str = row[3].get_str().unwrap_or_default();
        let aliases = if aliases_str.is_empty() {
            vec![]
        } else {
            aliases_str
                .split(',')
                .map(|s| s.trim().to_owned())
                .collect()
        };
        let created_at = crate::knowledge::parse_timestamp(row[4].get_str().unwrap_or_default())
            .unwrap_or_else(jiff::Timestamp::now);
        let updated_at = crate::knowledge::parse_timestamp(row[5].get_str().unwrap_or_default())
            .unwrap_or_else(jiff::Timestamp::now);

        entities.push(crate::knowledge::Entity {
            id,
            name,
            entity_type,
            aliases,
            created_at,
            updated_at,
        });
    }

    Ok(entities)
}

/// Query all relationships from the knowledge store.
#[cfg(feature = "mneme-engine")]
fn query_all_relationships(
    store: &crate::knowledge_store::KnowledgeStore,
) -> Result<Vec<crate::knowledge::Relationship>> {
    use std::collections::BTreeMap;

    let script = r"?[src, dst, relation, weight, created_at] := *relationships{src, dst, relation, weight, created_at}";
    let rows = store.run_query(script, BTreeMap::new())?;

    let mut relationships = Vec::new();
    for row in &rows.rows {
        if row.len() < 5 {
            continue;
        }
        let src = crate::id::EntityId::new_unchecked(row[0].get_str().unwrap_or_default());
        let dst = crate::id::EntityId::new_unchecked(row[1].get_str().unwrap_or_default());
        let relation = row[2].get_str().unwrap_or_default().to_owned();
        let weight = row[3].get_float().unwrap_or(0.0);
        let created_at = crate::knowledge::parse_timestamp(row[4].get_str().unwrap_or_default())
            .unwrap_or_else(jiff::Timestamp::now);

        relationships.push(crate::knowledge::Relationship {
            src,
            dst,
            relation,
            weight,
            created_at,
        });
    }

    Ok(relationships)
}

/// Scan a workspace directory, classifying files as text or binary.
fn scan_workspace(workspace_path: &Path) -> Result<WorkspaceData> {
    let mut files = HashMap::new();
    let mut binary_files = Vec::new();

    if !workspace_path.exists() {
        warn!(path = %workspace_path.display(), "workspace not found, exporting empty");
        return Ok(WorkspaceData {
            files,
            binary_files,
        });
    }

    walk_directory(
        workspace_path,
        workspace_path,
        &mut files,
        &mut binary_files,
    )?;

    Ok(WorkspaceData {
        files,
        binary_files,
    })
}

/// Recursive directory walk collecting text and binary file paths.
fn walk_directory(
    root: &Path,
    current: &Path,
    files: &mut HashMap<String, String>,
    binary_files: &mut Vec<String>,
) -> Result<()> {
    let entries = match std::fs::read_dir(current) {
        Ok(entries) => entries,
        Err(e) => {
            warn!(path = %current.display(), error = %e, "cannot read directory");
            return Ok(());
        }
    };

    for entry in entries {
        let entry = entry.context(error::IoSnafu {
            path: current.to_path_buf(),
        })?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .context(error::IoSnafu { path: path.clone() })?;

        if file_type.is_dir() {
            let dir_name = entry.file_name();
            let name = dir_name.to_string_lossy();
            if IGNORE_DIRS.contains(&name.as_ref()) {
                continue;
            }
            walk_directory(root, &path, files, binary_files)?;
            continue;
        }

        if !file_type.is_file() {
            continue;
        }

        let rel_path = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");

        let metadata = match std::fs::metadata(&path) {
            Ok(m) => m,
            Err(e) => {
                warn!(path = %path.display(), error = %e, "cannot stat file");
                binary_files.push(rel_path);
                continue;
            }
        };

        if metadata.len() > MAX_FILE_SIZE {
            binary_files.push(rel_path);
            continue;
        }

        if is_binary_path(&path) || is_binary_content(&path) {
            binary_files.push(rel_path);
        } else {
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    files.insert(rel_path, content);
                }
                Err(_) => {
                    binary_files.push(rel_path);
                }
            }
        }
    }

    Ok(())
}

/// Check if a file path has a known binary extension.
fn is_binary_path(path: &Path) -> bool {
    const BINARY_EXTENSIONS: &[&str] = &[
        "png", "jpg", "jpeg", "gif", "ico", "svg", "webp", "woff", "woff2", "ttf", "eot", "zip",
        "tar", "gz", "bz2", "xz", "pdf", "doc", "docx", "xlsx", "db", "sqlite", "sqlite3", "wasm",
        "so", "dylib", "exe", "dll", "o", "a",
    ];

    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| BINARY_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
}

/// Probe file content for null bytes indicating binary data.
fn is_binary_content(path: &Path) -> bool {
    use std::io::Read;

    #[expect(
        clippy::disallowed_methods,
        reason = "mneme filesystem operations access the embedded DB or model files; synchronous I/O is required in these contexts"
    )]
    let Ok(file) = std::fs::File::open(path) else {
        return true;
    };

    let mut buf = vec![0u8; BINARY_PROBE_SIZE];
    #[expect(
        clippy::as_conversions,
        reason = "usize→u64: BINARY_PROBE_SIZE is a small constant, fits in u64"
    )]
    let Ok(n) = file.take(BINARY_PROBE_SIZE as u64).read(&mut buf) else {
        return true;
    };

    #[expect(
        clippy::indexing_slicing,
        reason = "n comes from read() which guarantees n <= buf.len()"
    )]
    buf[..n].contains(&0)
}

/// Export a single session with all messages and notes.
fn export_session(
    store: &SessionStore,
    session: &crate::types::Session,
    opts: &ExportOptions,
) -> Result<ExportedSession> {
    let messages = get_all_messages(store, &session.id, opts.max_messages_per_session)?;
    let notes = store.get_notes(&session.id)?;

    let (working_state, distillation_priming) = get_session_json_fields(store, &session.id)?;

    Ok(ExportedSession {
        id: session.id.clone(),
        session_key: session.session_key.clone(),
        status: session.status.as_str().to_owned(),
        session_type: session.session_type.as_str().to_owned(),
        message_count: session.metrics.message_count,
        token_count_estimate: session.metrics.token_count_estimate,
        distillation_count: session.metrics.distillation_count,
        created_at: session.created_at.clone(),
        updated_at: session.updated_at.clone(),
        working_state,
        distillation_priming,
        notes: notes
            .into_iter()
            .map(|n| ExportedNote {
                category: n.category,
                content: n.content,
                created_at: n.created_at,
            })
            .collect(),
        messages,
    })
}

/// Get ALL messages for a session (including distilled) via raw SQL.
fn get_all_messages(
    store: &SessionStore,
    session_id: &str,
    max: usize,
) -> Result<Vec<ExportedMessage>> {
    let conn = store.conn();
    let sql = if max > 0 {
        "SELECT seq, role, content, token_estimate, is_distilled, created_at \
         FROM messages WHERE session_id = ?1 ORDER BY seq ASC LIMIT ?2"
    } else {
        "SELECT seq, role, content, token_estimate, is_distilled, created_at \
         FROM messages WHERE session_id = ?1 ORDER BY seq ASC"
    };

    let mut stmt = conn.prepare_cached(sql).context(error::DatabaseSnafu)?;

    let rows = if max > 0 {
        stmt.query_map(
            rusqlite::params![session_id, i64::try_from(max).unwrap_or(i64::MAX)],
            map_exported_message,
        )
        .context(error::DatabaseSnafu)?
    } else {
        stmt.query_map(rusqlite::params![session_id], map_exported_message)
            .context(error::DatabaseSnafu)?
    };

    let mut messages = Vec::new();
    for row in rows {
        messages.push(row.context(error::DatabaseSnafu)?);
    }
    Ok(messages)
}

/// Map a row to an `ExportedMessage`.
fn map_exported_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<ExportedMessage> {
    let distilled: i64 = row.get(4)?;
    Ok(ExportedMessage {
        seq: row.get(0)?,
        role: row.get(1)?,
        content: row.get(2)?,
        token_estimate: row.get(3)?,
        is_distilled: distilled != 0,
        created_at: row.get(5)?,
    })
}

/// Read `working_state` and `distillation_priming` TEXT columns as JSON.
fn get_session_json_fields(
    store: &SessionStore,
    session_id: &str,
) -> Result<(Option<serde_json::Value>, Option<serde_json::Value>)> {
    let conn = store.conn();
    let mut stmt = conn
        .prepare_cached("SELECT working_state, distillation_priming FROM sessions WHERE id = ?1")
        .context(error::DatabaseSnafu)?;

    let result = stmt
        .query_row([session_id], |row| {
            let ws: Option<String> = row.get(0)?;
            let dp: Option<String> = row.get(1)?;
            Ok((ws, dp))
        })
        .context(error::DatabaseSnafu)?;

    let working_state = result.0.and_then(|s| serde_json::from_str(&s).ok());
    let distillation_priming = result.1.and_then(|s| serde_json::from_str(&s).ok());

    Ok((working_state, distillation_priming))
}

#[cfg(test)]
#[path = "export_tests.rs"]
mod tests;
