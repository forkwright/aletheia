//! Agent export — build an `AgentFile` from session store and workspace.

use std::collections::HashMap;
use std::path::Path;

use snafu::ResultExt;
use tracing::{info, warn};

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
    })
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
        let dir = tempfile::tempdir().unwrap();
        let text_path = dir.path().join("text.txt");
        let bin_path = dir.path().join("data.bin");

        std::fs::write(&text_path, "hello world").unwrap();
        std::fs::write(&bin_path, b"\x00\x01\x02\x03").unwrap();

        assert!(!is_binary_content(&text_path));
        assert!(is_binary_content(&bin_path));
    }

    #[test]
    fn scan_empty_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let ws = scan_workspace(dir.path()).unwrap();
        assert!(ws.files.is_empty());
        assert!(ws.binary_files.is_empty());
    }

    #[test]
    fn scan_missing_workspace() {
        let ws = scan_workspace(Path::new("/nonexistent/path")).unwrap();
        assert!(ws.files.is_empty());
        assert!(ws.binary_files.is_empty());
    }

    #[test]
    fn scan_classifies_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("notes.md"), "# Notes").unwrap();
        std::fs::write(dir.path().join("data.bin"), b"\x00binary\x00").unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(dir.path().join(".git/HEAD"), "ref: refs/heads/main").unwrap();

        let ws = scan_workspace(dir.path()).unwrap();
        assert_eq!(ws.files.len(), 1);
        assert!(ws.files.contains_key("notes.md"));
        assert_eq!(ws.binary_files.len(), 1);
        assert!(ws.binary_files.contains(&"data.bin".to_owned()));
    }

    #[test]
    fn scan_skips_ignored_dirs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("node_modules")).unwrap();
        std::fs::write(dir.path().join("node_modules/package.json"), "{}").unwrap();
        std::fs::write(dir.path().join("readme.md"), "hello").unwrap();

        let ws = scan_workspace(dir.path()).unwrap();
        assert_eq!(ws.files.len(), 1);
        assert!(ws.files.contains_key("readme.md"));
    }

    #[test]
    fn export_with_sessions() {
        let store = test_store();
        store
            .create_session("ses-1", "alice", "main", None, None)
            .unwrap();
        store
            .append_message("ses-1", Role::User, "hello", None, None, 50)
            .unwrap();
        store
            .append_message("ses-1", Role::Assistant, "hi", None, None, 40)
            .unwrap();
        store.add_note("ses-1", "alice", "task", "testing").unwrap();

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("notes.md"), "# Test").unwrap();

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
        .unwrap();

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
            .unwrap();
        store
            .create_session("ses-archived", "bob", "old", None, None)
            .unwrap();
        store
            .update_session_status("ses-archived", SessionStatus::Archived)
            .unwrap();

        let dir = tempfile::tempdir().unwrap();
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
        .unwrap();
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
        .unwrap();
        assert_eq!(agent.sessions.len(), 2);
    }

    #[test]
    fn export_includes_distilled_messages() {
        let store = test_store();
        store
            .create_session("ses-1", "carol", "main", None, None)
            .unwrap();
        store
            .append_message("ses-1", Role::User, "old", None, None, 100)
            .unwrap();
        store
            .append_message("ses-1", Role::User, "new", None, None, 50)
            .unwrap();
        store.mark_messages_distilled("ses-1", &[1]).unwrap();

        let dir = tempfile::tempdir().unwrap();
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
        .unwrap();

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
            .unwrap();
        for i in 1..=10 {
            store
                .append_message("ses-1", Role::User, &format!("msg {i}"), None, None, 10)
                .unwrap();
        }

        let dir = tempfile::tempdir().unwrap();
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
        .unwrap();

        assert_eq!(agent.sessions[0].messages.len(), 3);
        assert_eq!(agent.sessions[0].messages[0].content, "msg 1");
    }
}
