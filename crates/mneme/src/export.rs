//! Agent export — build an `AgentFile` from session store and workspace.

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
/// simple types here — mneme never touches taxis.
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
    // Query all current facts (use a far-future timestamp to capture everything)
    let facts = store
        .query_facts(nous_id, "9999-01-01T00:00:00Z", 100_000)
        .ok()
        .unwrap_or_default();

    // Query all entities via Datalog
    let entities = query_all_entities(store).unwrap_or_default();

    // Query all relationships via Datalog
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
            path: current.display().to_string(),
        })?;
        let path = entry.path();
        let file_type = entry.file_type().context(error::IoSnafu {
            path: path.display().to_string(),
        })?;

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

    let Ok(file) = std::fs::File::open(path) else {
        return true;
    };

    let mut buf = vec![0u8; BINARY_PROBE_SIZE];
    let Ok(n) = file.take(BINARY_PROBE_SIZE as u64).read(&mut buf) else {
        return true;
    };

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
        message_count: session.message_count,
        token_count_estimate: session.token_count_estimate,
        distillation_count: session.distillation_count,
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
mod tests {
    use super::*;
    use crate::store::SessionStore;
    use crate::types::Role;

    fn test_store() -> SessionStore {
        SessionStore::open_in_memory().expect("open in-memory store")
    }

    #[test]
    fn binary_path_detection() {
        assert!(is_binary_path(Path::new("avatar.png")));
        assert!(is_binary_path(Path::new("data.sqlite")));
        assert!(is_binary_path(Path::new("archive.tar.gz")));
        assert!(!is_binary_path(Path::new("notes.md")));
        assert!(!is_binary_path(Path::new("config.yaml")));
        assert!(!is_binary_path(Path::new("Makefile")));
    }

    #[test]
    fn binary_content_detection() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let text_path = dir.path().join("text.txt");
        let bin_path = dir.path().join("data.bin");

        std::fs::write(&text_path, "hello world").expect("write text file");
        std::fs::write(&bin_path, b"\x00\x01\x02\x03").expect("write binary file");

        assert!(!is_binary_content(&text_path));
        assert!(is_binary_content(&bin_path));
    }

    #[test]
    fn scan_empty_workspace() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let ws = scan_workspace(dir.path()).expect("scan empty workspace");
        assert!(ws.files.is_empty());
        assert!(ws.binary_files.is_empty());
    }

    #[test]
    fn scan_missing_workspace() {
        let ws = scan_workspace(Path::new("/nonexistent/path")).expect("scan missing workspace returns empty");
        assert!(ws.files.is_empty());
        assert!(ws.binary_files.is_empty());
    }

    #[test]
    fn scan_classifies_files() {
        let dir = tempfile::tempdir().expect("create tempdir");
        std::fs::write(dir.path().join("notes.md"), "# Notes").expect("write notes.md");
        std::fs::write(dir.path().join("data.bin"), b"\x00binary\x00").expect("write data.bin");
        std::fs::create_dir(dir.path().join(".git")).expect("create .git dir");
        std::fs::write(dir.path().join(".git/HEAD"), "ref: refs/heads/main").expect("write .git/HEAD");

        let ws = scan_workspace(dir.path()).expect("scan workspace");
        assert_eq!(ws.files.len(), 1);
        assert!(ws.files.contains_key("notes.md"));
        assert_eq!(ws.binary_files.len(), 1);
        assert!(ws.binary_files.contains(&"data.bin".to_owned()));
    }

    #[test]
    fn scan_skips_ignored_dirs() {
        let dir = tempfile::tempdir().expect("create tempdir");
        std::fs::create_dir(dir.path().join("node_modules")).expect("create node_modules dir");
        std::fs::write(dir.path().join("node_modules/package.json"), "{}").expect("write package.json");
        std::fs::write(dir.path().join("readme.md"), "hello").expect("write readme.md");

        let ws = scan_workspace(dir.path()).expect("scan workspace");
        assert_eq!(ws.files.len(), 1);
        assert!(ws.files.contains_key("readme.md"));
    }

    #[test]
    fn export_with_sessions() {
        let store = test_store();
        store
            .create_session("ses-1", "alice", "main", None, None)
            .expect("create session");
        store
            .append_message("ses-1", Role::User, "hello", None, None, 50)
            .expect("append user message");
        store
            .append_message("ses-1", Role::Assistant, "hi", None, None, 40)
            .expect("append assistant message");
        store.add_note("ses-1", "alice", "task", "testing").expect("add note");

        let dir = tempfile::tempdir().expect("create tempdir");
        std::fs::write(dir.path().join("notes.md"), "# Test").expect("write notes.md");

        let opts = ExportOptions::default();
        let agent = export_agent(
            "alice",
            Some("Alice"),
            Some("claude-sonnet-4-6"),
            serde_json::json!({}),
            &store,
            dir.path(),
            &opts,
        )
        .expect("export agent");

        assert_eq!(agent.version, 1);
        assert_eq!(agent.nous.id, "alice");
        assert_eq!(agent.nous.name.as_deref(), Some("Alice"));
        assert_eq!(agent.sessions.len(), 1);
        assert_eq!(agent.sessions[0].messages.len(), 2);
        assert_eq!(agent.sessions[0].notes.len(), 1);
        assert_eq!(agent.workspace.files.len(), 1);
    }

    #[test]
    fn export_filters_archived_by_default() {
        let store = test_store();
        store
            .create_session("ses-active", "bob", "main", None, None)
            .expect("create active session");
        store
            .create_session("ses-archived", "bob", "old", None, None)
            .expect("create archived session");
        store
            .update_session_status("ses-archived", SessionStatus::Archived)
            .expect("archive session");

        let dir = tempfile::tempdir().expect("create tempdir");
        let opts = ExportOptions::default();
        let agent = export_agent(
            "bob",
            None,
            None,
            serde_json::json!({}),
            &store,
            dir.path(),
            &opts,
        )
        .expect("export agent without archived");
        assert_eq!(agent.sessions.len(), 1);

        let opts_with_archived = ExportOptions {
            include_archived: true,
            ..Default::default()
        };
        let agent = export_agent(
            "bob",
            None,
            None,
            serde_json::json!({}),
            &store,
            dir.path(),
            &opts_with_archived,
        )
        .expect("export agent with archived");
        assert_eq!(agent.sessions.len(), 2);
    }

    #[test]
    fn export_includes_distilled_messages() {
        let store = test_store();
        store
            .create_session("ses-1", "carol", "main", None, None)
            .expect("create session");
        store
            .append_message("ses-1", Role::User, "old", None, None, 100)
            .expect("append old message");
        store
            .append_message("ses-1", Role::User, "new", None, None, 50)
            .expect("append new message");
        store.mark_messages_distilled("ses-1", &[1]).expect("mark messages distilled");

        let dir = tempfile::tempdir().expect("create tempdir");
        let opts = ExportOptions::default();
        let agent = export_agent(
            "carol",
            None,
            None,
            serde_json::json!({}),
            &store,
            dir.path(),
            &opts,
        )
        .expect("export agent with distilled messages");

        // Both messages exported, including the distilled one
        assert_eq!(agent.sessions[0].messages.len(), 2);
        assert!(agent.sessions[0].messages[0].is_distilled);
        assert!(!agent.sessions[0].messages[1].is_distilled);
    }

    #[test]
    fn export_message_limit() {
        let store = test_store();
        store
            .create_session("ses-1", "dave", "main", None, None)
            .expect("create session");
        for i in 1..=10 {
            store
                .append_message("ses-1", Role::User, &format!("msg {i}"), None, None, 10)
                .expect("append message");
        }

        let dir = tempfile::tempdir().expect("create tempdir");
        let opts = ExportOptions {
            max_messages_per_session: 3,
            include_archived: false,
        };
        let agent = export_agent(
            "dave",
            None,
            None,
            serde_json::json!({}),
            &store,
            dir.path(),
            &opts,
        )
        .expect("export agent with message limit");

        assert_eq!(agent.sessions[0].messages.len(), 3);
        assert_eq!(agent.sessions[0].messages[0].content, "msg 1");
    }

    #[test]
    fn export_empty_agent() {
        let store = test_store();
        let dir = tempfile::tempdir().expect("create tempdir");
        let opts = ExportOptions::default();

        let agent = export_agent(
            "nobody",
            None,
            None,
            serde_json::json!({}),
            &store,
            dir.path(),
            &opts,
        )
        .expect("export empty agent");

        assert_eq!(agent.sessions.len(), 0);
        assert!(agent.workspace.files.is_empty());
        assert!(agent.workspace.binary_files.is_empty());
        assert_eq!(agent.nous.id, "nobody");
        assert!(agent.memory.is_none());

        let json = serde_json::to_string(&agent).expect("serialize agent to JSON");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse JSON value");
        assert!(parsed.is_object(), "empty export produces valid JSON");
    }

    #[test]
    fn export_preserves_timestamps() {
        let store = test_store();
        store
            .create_session("ses-ts", "ts-agent", "main", None, None)
            .expect("create session");
        store
            .append_message("ses-ts", Role::User, "time test", None, None, 30)
            .expect("append message");

        let dir = tempfile::tempdir().expect("create tempdir");
        let agent = export_agent(
            "ts-agent",
            None,
            None,
            serde_json::json!({}),
            &store,
            dir.path(),
            &ExportOptions::default(),
        )
        .expect("export agent");

        let session = &agent.sessions[0];
        assert!(!session.created_at.is_empty(), "created_at must be set");
        assert!(!session.updated_at.is_empty(), "updated_at must be set");
        assert!(
            !session.messages[0].created_at.is_empty(),
            "message created_at must be set"
        );

        let json = serde_json::to_string(&agent).expect("serialize agent to JSON");
        let restored: crate::portability::AgentFile = serde_json::from_str(&json).expect("deserialize agent from JSON");
        assert_eq!(restored.sessions[0].created_at, session.created_at);
        assert_eq!(restored.sessions[0].updated_at, session.updated_at);
        assert_eq!(
            restored.sessions[0].messages[0].created_at,
            session.messages[0].created_at
        );
    }

    #[test]
    fn export_preserves_unicode() {
        let store = test_store();
        store
            .create_session("ses-uni", "uni-agent", "main", None, None)
            .expect("create session");

        let emoji = "Hello 🌍🔥 world";
        let cjk = "你好世界 こんにちは";
        let rtl = "مرحبا بالعالم";
        let mixed = format!("{emoji} {cjk} {rtl}");

        store
            .append_message("ses-uni", Role::User, &mixed, None, None, 100)
            .expect("append unicode message");
        store
            .add_note("ses-uni", "uni-agent", "context", &mixed)
            .expect("add unicode note");

        let dir = tempfile::tempdir().expect("create tempdir");
        let unicode_file = "日本語.txt";
        std::fs::write(dir.path().join(unicode_file), &mixed).expect("write unicode file");

        let agent = export_agent(
            "uni-agent",
            Some("Ünïcödé Àgënt"),
            None,
            serde_json::json!({"note": cjk}),
            &store,
            dir.path(),
            &ExportOptions::default(),
        )
        .expect("export unicode agent");

        assert_eq!(agent.sessions[0].messages[0].content, mixed);
        assert_eq!(agent.sessions[0].notes[0].content, mixed);
        assert_eq!(agent.nous.name.as_deref(), Some("Ünïcödé Àgënt"));

        let json = serde_json::to_string_pretty(&agent).expect("serialize unicode agent to JSON");
        let restored: crate::portability::AgentFile = serde_json::from_str(&json).expect("deserialize unicode agent from JSON");
        assert_eq!(restored.sessions[0].messages[0].content, mixed);
        assert_eq!(restored.sessions[0].notes[0].content, mixed);
    }

    #[test]
    fn export_empty_store() {
        let store = test_store();
        let dir = tempfile::tempdir().expect("create tempdir");
        let opts = ExportOptions::default();

        let agent = export_agent(
            "empty-agent",
            None,
            None,
            serde_json::json!({}),
            &store,
            dir.path(),
            &opts,
        )
        .expect("export empty store agent");

        assert_eq!(agent.sessions.len(), 0);
        assert_eq!(agent.nous.id, "empty-agent");

        let json = serde_json::to_string(&agent).expect("serialize agent to JSON");
        let restored: crate::portability::AgentFile = serde_json::from_str(&json).expect("deserialize agent from JSON");
        assert_eq!(restored.sessions.len(), 0);
    }

    #[test]
    fn export_with_message_limit() {
        let store = test_store();
        store
            .create_session("ses-lim", "limiter", "main", None, None)
            .expect("create session");
        for i in 1..=20 {
            store
                .append_message("ses-lim", Role::User, &format!("msg {i}"), None, None, 10)
                .expect("append message");
        }

        let dir = tempfile::tempdir().expect("create tempdir");

        let opts_limited = ExportOptions {
            max_messages_per_session: 5,
            include_archived: false,
        };
        let agent = export_agent(
            "limiter",
            None,
            None,
            serde_json::json!({}),
            &store,
            dir.path(),
            &opts_limited,
        )
        .expect("export agent with limit of 5");
        assert_eq!(agent.sessions[0].messages.len(), 5);

        let opts_unlimited = ExportOptions {
            max_messages_per_session: 0,
            include_archived: false,
        };
        let agent_all = export_agent(
            "limiter",
            None,
            None,
            serde_json::json!({}),
            &store,
            dir.path(),
            &opts_unlimited,
        )
        .expect("export agent with no message limit");
        assert_eq!(agent_all.sessions[0].messages.len(), 20);
    }

    #[test]
    fn is_binary_file_detects_binary() {
        let known_binaries = [
            "image.png",
            "photo.jpg",
            "archive.zip",
            "data.sqlite",
            "font.woff2",
            "app.exe",
            "lib.so",
            "module.wasm",
        ];
        for name in &known_binaries {
            assert!(
                is_binary_path(Path::new(name)),
                "{name} should be detected as binary"
            );
        }
    }

    #[test]
    fn is_binary_file_allows_text() {
        let text_files = [
            "readme.md",
            "config.yaml",
            "main.rs",
            "index.html",
            "style.css",
            "data.json",
            "script.py",
            "Makefile",
        ];
        for name in &text_files {
            assert!(
                !is_binary_path(Path::new(name)),
                "{name} should not be detected as binary"
            );
        }
    }

    #[test]
    fn export_preserves_session_metadata() {
        let store = test_store();
        store
            .create_session("ses-meta", "meta-agent", "main", None, None)
            .expect("create session");
        store
            .append_message("ses-meta", Role::User, "hello", None, None, 42)
            .expect("append message");
        store
            .add_note("ses-meta", "meta-agent", "context", "important note")
            .expect("add note");

        let dir = tempfile::tempdir().expect("create tempdir");
        let agent = export_agent(
            "meta-agent",
            Some("Meta"),
            Some("test-model"),
            serde_json::json!({"key": "value"}),
            &store,
            dir.path(),
            &ExportOptions::default(),
        )
        .expect("export agent with session metadata");

        let session = &agent.sessions[0];
        assert_eq!(session.id, "ses-meta");
        assert_eq!(session.session_key, "main");
        assert_eq!(session.status, "active");
        assert_eq!(session.message_count, 1);
        assert!(!session.created_at.is_empty());
        assert!(!session.updated_at.is_empty());
        assert_eq!(session.notes.len(), 1);
        assert_eq!(session.notes[0].category, "context");
        assert_eq!(session.notes[0].content, "important note");
    }

    #[test]
    fn export_filters_archived_sessions() {
        let store = test_store();
        store
            .create_session("ses-a", "filter-agent", "main", None, None)
            .expect("create active session");
        store
            .create_session("ses-b", "filter-agent", "old", None, None)
            .expect("create session to archive");
        store
            .update_session_status("ses-b", SessionStatus::Archived)
            .expect("archive session");

        let dir = tempfile::tempdir().expect("create tempdir");

        let opts_default = ExportOptions::default();
        let agent = export_agent(
            "filter-agent",
            None,
            None,
            serde_json::json!({}),
            &store,
            dir.path(),
            &opts_default,
        )
        .expect("export agent without archived");
        assert_eq!(
            agent.sessions.len(),
            1,
            "archived sessions excluded by default"
        );
        assert_eq!(agent.sessions[0].id, "ses-a");

        let opts_include = ExportOptions {
            include_archived: true,
            ..Default::default()
        };
        let agent_all = export_agent(
            "filter-agent",
            None,
            None,
            serde_json::json!({}),
            &store,
            dir.path(),
            &opts_include,
        )
        .expect("export agent with archived");
        assert_eq!(
            agent_all.sessions.len(),
            2,
            "archived included when opted in"
        );
    }

    #[test]
    fn scan_workspace_nested_structure() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let sub = dir.path().join("sub/deep");
        std::fs::create_dir_all(&sub).expect("create nested directories");
        std::fs::write(dir.path().join("root.txt"), "root").expect("write root.txt");
        std::fs::write(sub.join("nested.md"), "nested").expect("write nested.md");

        let ws = scan_workspace(dir.path()).expect("scan nested workspace");
        assert_eq!(ws.files.len(), 2);
        assert!(ws.files.contains_key("root.txt"));
        assert!(ws.files.contains_key("sub/deep/nested.md"));
    }
}
