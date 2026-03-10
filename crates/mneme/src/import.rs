//! Agent import — restore an agent from a portable `AgentFile`.

use std::path::Path;

use snafu::{ResultExt, ensure};
use tracing::{info, instrument, warn};

use crate::error::{self, Result};
use crate::portability::{AGENT_FILE_VERSION, AgentFile};
use crate::store::SessionStore;

/// Options controlling import behavior.
#[derive(Debug, Clone, Default)]
pub struct ImportOptions {
    /// Skip importing session history.
    pub skip_sessions: bool,
    /// Skip restoring workspace files.
    pub skip_workspace: bool,
    /// Override the target agent ID.
    pub target_nous_id: Option<String>,
    /// Overwrite existing workspace files.
    pub force: bool,
}

/// Summary of what was imported.
#[derive(Debug, Clone)]
pub struct ImportResult {
    /// Agent ID the data was imported under.
    pub nous_id: String,
    /// Number of workspace files restored.
    pub files_restored: u32,
    /// Number of sessions created.
    pub sessions_imported: u32,
    /// Total messages inserted.
    pub messages_imported: u32,
    /// Total notes inserted.
    pub notes_imported: u32,
}

/// Import an agent from a portable `AgentFile`.
///
/// `id_generator` produces new session IDs — the caller provides this because
/// mneme doesn't depend on `ulid`.
///
/// # Errors
///
/// Returns errors for unsupported versions, path traversal attempts, or store/IO failures.
#[instrument(skip(agent_file, store, id_generator))]
pub fn import_agent(
    agent_file: &AgentFile,
    store: &SessionStore,
    workspace_path: &Path,
    id_generator: &dyn Fn() -> String,
    opts: &ImportOptions,
) -> Result<ImportResult> {
    ensure!(
        agent_file.version == AGENT_FILE_VERSION,
        error::UnsupportedVersionSnafu {
            version: agent_file.version,
        }
    );

    let nous_id = opts
        .target_nous_id
        .as_deref()
        .unwrap_or(&agent_file.nous.id);

    let mut result = ImportResult {
        nous_id: nous_id.to_owned(),
        files_restored: 0,
        sessions_imported: 0,
        messages_imported: 0,
        notes_imported: 0,
    };

    if !opts.skip_workspace {
        result.files_restored =
            restore_workspace(&agent_file.workspace.files, workspace_path, opts.force)?;
    }

    if !opts.skip_sessions {
        import_sessions(agent_file, store, nous_id, id_generator, &mut result)?;
    }

    if let Some(ref memory) = agent_file.memory {
        let vectors = memory.vectors.as_ref().map_or(0, Vec::len);
        let graph = memory.graph.is_some();
        if vectors > 0 || graph {
            info!(
                vectors,
                graph, "memory data present but requires sidecar — skipped"
            );
        }
    }

    if let Some(ref knowledge) = agent_file.knowledge {
        info!(
            facts = knowledge.facts.len(),
            entities = knowledge.entities.len(),
            relationships = knowledge.relationships.len(),
            "knowledge data present — import requires knowledge store (skipped in session import)"
        );
    }

    info!(
        nous_id,
        files = result.files_restored,
        sessions = result.sessions_imported,
        messages = result.messages_imported,
        notes = result.notes_imported,
        "agent imported"
    );

    Ok(result)
}

/// Validate a relative file path for safety.
fn validate_relative_path(rel_path: &str) -> bool {
    if rel_path.is_empty() {
        return false;
    }

    // Reject absolute paths
    if rel_path.starts_with('/') || rel_path.starts_with('\\') {
        return false;
    }

    // Reject Windows drive letters
    if rel_path.len() >= 2 && rel_path.as_bytes()[1] == b':' {
        return false;
    }

    // Reject protocol prefixes
    if rel_path.contains("://") {
        return false;
    }

    // Reject path traversal via .. components
    for component in rel_path.split(['/', '\\']) {
        if component == ".." {
            return false;
        }
    }

    true
}

/// Restore workspace files to disk.
fn restore_workspace(
    files: &std::collections::HashMap<String, String>,
    workspace_path: &Path,
    force: bool,
) -> Result<u32> {
    let mut count = 0;

    for (rel_path, content) in files {
        ensure!(
            validate_relative_path(rel_path),
            error::UnsafePathSnafu {
                path: rel_path.clone(),
            }
        );

        let full_path = workspace_path.join(rel_path);

        if full_path.exists() && !force {
            warn!(path = %rel_path, "skipping existing file (use --force to overwrite)");
            continue;
        }

        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent).context(error::IoSnafu {
                path: parent.display().to_string(),
            })?;
        }

        std::fs::write(&full_path, content).context(error::IoSnafu {
            path: full_path.display().to_string(),
        })?;

        count += 1;
    }

    Ok(count)
}

/// Import all sessions, messages, and notes from the agent file.
fn import_sessions(
    agent_file: &AgentFile,
    store: &SessionStore,
    nous_id: &str,
    id_generator: &dyn Fn() -> String,
    result: &mut ImportResult,
) -> Result<()> {
    let conn = store.conn();
    let timestamp = jiff::Timestamp::now().strftime("%Y%m%dT%H%M%S").to_string();

    for exported in &agent_file.sessions {
        let new_id = id_generator();
        let session_key = format!("{}-import-{timestamp}", exported.session_key);

        conn.execute(
            "INSERT INTO sessions (id, nous_id, session_key, status, session_type, \
             token_count_estimate, message_count, distillation_count, \
             working_state, distillation_priming, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            rusqlite::params![
                new_id,
                nous_id,
                session_key,
                exported.status,
                exported.session_type,
                exported.token_count_estimate,
                exported.message_count,
                exported.distillation_count,
                exported
                    .working_state
                    .as_ref()
                    .map(serde_json::Value::to_string),
                exported
                    .distillation_priming
                    .as_ref()
                    .map(serde_json::Value::to_string),
                exported.created_at,
                exported.updated_at,
            ],
        )
        .context(error::DatabaseSnafu)?;

        result.sessions_imported += 1;

        // Import messages in sequence order
        let mut sorted_messages = exported.messages.clone();
        sorted_messages.sort_by_key(|m| m.seq);

        for msg in &sorted_messages {
            conn.execute(
                "INSERT INTO messages (session_id, seq, role, content, token_estimate, \
                 is_distilled, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    new_id,
                    msg.seq,
                    msg.role,
                    msg.content,
                    msg.token_estimate,
                    i64::from(msg.is_distilled),
                    msg.created_at,
                ],
            )
            .context(error::DatabaseSnafu)?;

            result.messages_imported += 1;
        }

        // Import notes
        let valid_categories = crate::schema::VALID_CATEGORIES;
        for note in &exported.notes {
            let category = if valid_categories.contains(&note.category.as_str()) {
                &note.category
            } else {
                "context"
            };
            store.add_note(&new_id, nous_id, category, &note.content)?;
            result.notes_imported += 1;
        }
    }

    Ok(())
}

/// Import knowledge graph data from an `AgentFile` into a knowledge store.
///
/// # Errors
///
/// Returns errors if fact/entity/relationship insertion fails.
#[cfg(feature = "mneme-engine")]
#[instrument(skip(knowledge, store))]
pub fn import_knowledge(
    knowledge: &crate::portability::KnowledgeExport,
    store: &crate::knowledge_store::KnowledgeStore,
) -> Result<KnowledgeImportResult> {
    let mut result = KnowledgeImportResult::default();

    for entity in &knowledge.entities {
        if let Err(e) = store.insert_entity(entity) {
            warn!(entity_id = %entity.id, error = %e, "failed to import entity");
            continue;
        }
        result.entities_imported += 1;
    }

    for rel in &knowledge.relationships {
        if let Err(e) = store.insert_relationship(rel) {
            warn!(src = %rel.src, dst = %rel.dst, error = %e, "failed to import relationship");
            continue;
        }
        result.relationships_imported += 1;
    }

    for fact in &knowledge.facts {
        if let Err(e) = store.insert_fact(fact) {
            warn!(fact_id = %fact.id, error = %e, "failed to import fact");
            continue;
        }
        result.facts_imported += 1;
    }

    info!(
        facts = result.facts_imported,
        entities = result.entities_imported,
        relationships = result.relationships_imported,
        "knowledge imported"
    );

    Ok(result)
}

/// Summary of knowledge graph import results.
#[cfg(feature = "mneme-engine")]
#[derive(Debug, Clone, Default)]
pub struct KnowledgeImportResult {
    /// Number of facts successfully imported.
    pub facts_imported: usize,
    /// Number of entities successfully imported.
    pub entities_imported: usize,
    /// Number of relationships successfully imported.
    pub relationships_imported: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::{ExportOptions, export_agent};
    use crate::portability::*;
    use crate::store::SessionStore;
    use crate::types::Role;
    use std::collections::HashMap;

    fn test_store() -> SessionStore {
        SessionStore::open_in_memory().expect("open in-memory store")
    }

    fn counter_id_gen() -> Box<dyn Fn() -> String> {
        let counter = std::sync::atomic::AtomicU64::new(1);
        Box::new(move || {
            let n = counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            format!("import-{n}")
        })
    }

    fn minimal_agent_file() -> AgentFile {
        AgentFile {
            version: 1,
            exported_at: "2026-03-05T12:00:00Z".to_owned(),
            generator: "test".to_owned(),
            nous: NousInfo {
                id: "alice".to_owned(),
                name: Some("Alice".to_owned()),
                model: None,
                config: serde_json::json!({}),
            },
            workspace: WorkspaceData {
                files: HashMap::from([("notes.md".to_owned(), "# Notes\n".to_owned())]),
                binary_files: vec![],
            },
            sessions: vec![ExportedSession {
                id: "ses-orig".to_owned(),
                session_key: "main".to_owned(),
                status: "active".to_owned(),
                session_type: "primary".to_owned(),
                message_count: 2,
                token_count_estimate: 100,
                distillation_count: 0,
                created_at: "2026-03-05T10:00:00Z".to_owned(),
                updated_at: "2026-03-05T11:00:00Z".to_owned(),
                working_state: None,
                distillation_priming: None,
                notes: vec![ExportedNote {
                    category: "task".to_owned(),
                    content: "testing import".to_owned(),
                    created_at: "2026-03-05T10:30:00Z".to_owned(),
                }],
                messages: vec![
                    ExportedMessage {
                        role: "user".to_owned(),
                        content: "hello".to_owned(),
                        seq: 1,
                        token_estimate: 50,
                        is_distilled: false,
                        created_at: "2026-03-05T10:00:00Z".to_owned(),
                    },
                    ExportedMessage {
                        role: "assistant".to_owned(),
                        content: "hi".to_owned(),
                        seq: 2,
                        token_estimate: 50,
                        is_distilled: false,
                        created_at: "2026-03-05T10:00:01Z".to_owned(),
                    },
                ],
            }],
            memory: None,
            knowledge: None,
        }
    }

    #[test]
    fn path_traversal_rejected() {
        assert!(!validate_relative_path("../etc/passwd"));
        assert!(!validate_relative_path("foo/../../etc/shadow"));
        assert!(!validate_relative_path("/absolute/path"));
        assert!(!validate_relative_path("\\windows\\path"));
        assert!(!validate_relative_path("C:\\Users\\evil"));
        assert!(!validate_relative_path("file:///etc/passwd"));
        assert!(!validate_relative_path(""));
    }

    #[test]
    fn valid_paths_accepted() {
        assert!(validate_relative_path("notes.md"));
        assert!(validate_relative_path("memory/2026-03-05.md"));
        assert!(validate_relative_path("sub/dir/file.txt"));
        assert!(validate_relative_path(".env"));
    }

    #[test]
    fn rejects_unsupported_version() {
        let store = test_store();
        let dir = tempfile::tempdir().expect("create temp dir");
        let mut agent = minimal_agent_file();
        agent.version = 99;

        let id_gen = counter_id_gen();
        let result = import_agent(
            &agent,
            &store,
            dir.path(),
            &*id_gen,
            &ImportOptions::default(),
        );

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("unsupported agent file version: 99"));
    }

    #[test]
    fn import_restores_workspace_files() {
        let store = test_store();
        let dir = tempfile::tempdir().expect("create temp dir");
        let agent = minimal_agent_file();

        let id_gen = counter_id_gen();
        let result = import_agent(
            &agent,
            &store,
            dir.path(),
            &*id_gen,
            &ImportOptions {
                skip_sessions: true,
                ..Default::default()
            },
        )
        .expect("import_agent should succeed");

        assert_eq!(result.files_restored, 1);
        let content = std::fs::read_to_string(dir.path().join("notes.md"))
            .expect("notes.md should be written");
        assert_eq!(content, "# Notes\n");
    }

    #[test]
    fn import_skips_existing_without_force() {
        let store = test_store();
        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::write(dir.path().join("notes.md"), "original").expect("write existing notes.md");

        let agent = minimal_agent_file();
        let id_gen = counter_id_gen();
        let result = import_agent(
            &agent,
            &store,
            dir.path(),
            &*id_gen,
            &ImportOptions {
                skip_sessions: true,
                ..Default::default()
            },
        )
        .expect("import_agent should succeed");

        assert_eq!(result.files_restored, 0);
        let content = std::fs::read_to_string(dir.path().join("notes.md"))
            .expect("notes.md should be readable");
        assert_eq!(content, "original");
    }

    #[test]
    fn import_overwrites_with_force() {
        let store = test_store();
        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::write(dir.path().join("notes.md"), "original").expect("write existing notes.md");

        let agent = minimal_agent_file();
        let id_gen = counter_id_gen();
        let result = import_agent(
            &agent,
            &store,
            dir.path(),
            &*id_gen,
            &ImportOptions {
                skip_sessions: true,
                force: true,
                ..Default::default()
            },
        )
        .expect("import_agent with force should succeed");

        assert_eq!(result.files_restored, 1);
        let content = std::fs::read_to_string(dir.path().join("notes.md"))
            .expect("notes.md should be overwritten");
        assert_eq!(content, "# Notes\n");
    }

    #[test]
    fn import_creates_sessions_and_messages() {
        let store = test_store();
        let dir = tempfile::tempdir().expect("create temp dir");
        let agent = minimal_agent_file();

        let id_gen = counter_id_gen();
        let result = import_agent(
            &agent,
            &store,
            dir.path(),
            &*id_gen,
            &ImportOptions::default(),
        )
        .expect("import_agent should succeed");

        assert_eq!(result.sessions_imported, 1);
        assert_eq!(result.messages_imported, 2);
        assert_eq!(result.notes_imported, 1);

        let sessions = store
            .list_sessions(Some("alice"))
            .expect("list_sessions should succeed");
        assert_eq!(sessions.len(), 1);
        assert!(sessions[0].session_key.starts_with("main-import-"));
    }

    #[test]
    fn import_with_target_id_override() {
        let store = test_store();
        let dir = tempfile::tempdir().expect("create temp dir");
        let agent = minimal_agent_file();

        let id_gen = counter_id_gen();
        let result = import_agent(
            &agent,
            &store,
            dir.path(),
            &*id_gen,
            &ImportOptions {
                target_nous_id: Some("bob".to_owned()),
                ..Default::default()
            },
        )
        .expect("import_agent with target_nous_id should succeed");

        assert_eq!(result.nous_id, "bob");
        let sessions = store
            .list_sessions(Some("bob"))
            .expect("list_sessions for bob should succeed");
        assert_eq!(sessions.len(), 1);
    }

    #[test]
    fn import_skip_sessions_flag() {
        let store = test_store();
        let dir = tempfile::tempdir().expect("create temp dir");
        let agent = minimal_agent_file();

        let id_gen = counter_id_gen();
        let result = import_agent(
            &agent,
            &store,
            dir.path(),
            &*id_gen,
            &ImportOptions {
                skip_sessions: true,
                ..Default::default()
            },
        )
        .expect("import_agent with skip_sessions should succeed");

        assert_eq!(result.sessions_imported, 0);
        assert_eq!(result.messages_imported, 0);
        assert_eq!(result.files_restored, 1);
    }

    #[test]
    fn import_skip_workspace_flag() {
        let store = test_store();
        let dir = tempfile::tempdir().expect("create temp dir");
        let agent = minimal_agent_file();

        let id_gen = counter_id_gen();
        let result = import_agent(
            &agent,
            &store,
            dir.path(),
            &*id_gen,
            &ImportOptions {
                skip_workspace: true,
                ..Default::default()
            },
        )
        .expect("import_agent with skip_workspace should succeed");

        assert_eq!(result.files_restored, 0);
        assert_eq!(result.sessions_imported, 1);
        assert!(!dir.path().join("notes.md").exists());
    }

    #[test]
    fn import_rejects_path_traversal() {
        let store = test_store();
        let dir = tempfile::tempdir().expect("create temp dir");
        let mut agent = minimal_agent_file();
        agent.workspace.files =
            HashMap::from([("../../../etc/passwd".to_owned(), "evil".to_owned())]);

        let id_gen = counter_id_gen();
        let result = import_agent(
            &agent,
            &store,
            dir.path(),
            &*id_gen,
            &ImportOptions::default(),
        );

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("unsafe path"));
    }

    #[test]
    fn import_validates_note_categories() {
        let store = test_store();
        let dir = tempfile::tempdir().expect("create temp dir");
        let mut agent = minimal_agent_file();
        agent.sessions[0].notes.push(ExportedNote {
            category: "invalid_category".to_owned(),
            content: "should default to context".to_owned(),
            created_at: "2026-03-05T10:30:00Z".to_owned(),
        });

        let id_gen = counter_id_gen();
        let result = import_agent(
            &agent,
            &store,
            dir.path(),
            &*id_gen,
            &ImportOptions::default(),
        )
        .expect("import_agent should succeed with invalid category defaulted");

        assert_eq!(result.notes_imported, 2);
    }

    #[test]
    fn export_import_roundtrip() {
        let store = test_store();
        store
            .create_session("ses-1", "eve", "main", None, None)
            .expect("create session ses-1");
        store
            .append_message("ses-1", Role::User, "hello", None, None, 50)
            .expect("append user message");
        store
            .append_message("ses-1", Role::Assistant, "hi back", None, None, 40)
            .expect("append assistant message");
        store
            .add_note("ses-1", "eve", "task", "roundtrip test")
            .expect("add note");

        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::write(dir.path().join("readme.md"), "# Hello").expect("write readme.md");

        let exported = export_agent(
            "eve",
            Some("Eve"),
            None,
            serde_json::json!({}),
            &store,
            dir.path(),
            &ExportOptions::default(),
        )
        .expect("export_agent should succeed");

        // Serialize and deserialize to simulate file I/O
        let json =
            serde_json::to_string_pretty(&exported).expect("serialize exported agent to JSON");
        let imported: AgentFile =
            serde_json::from_str(&json).expect("deserialize agent file from JSON");

        // Import into fresh store under different ID
        let import_store = test_store();
        let import_dir = tempfile::tempdir().expect("create import temp dir");
        let id_gen = counter_id_gen();
        let result = import_agent(
            &imported,
            &import_store,
            import_dir.path(),
            &*id_gen,
            &ImportOptions {
                target_nous_id: Some("eve-clone".to_owned()),
                ..Default::default()
            },
        )
        .expect("import_agent roundtrip should succeed");

        assert_eq!(result.nous_id, "eve-clone");
        assert_eq!(result.files_restored, 1);
        assert_eq!(result.sessions_imported, 1);
        assert_eq!(result.messages_imported, 2);
        assert_eq!(result.notes_imported, 1);

        let content = std::fs::read_to_string(import_dir.path().join("readme.md"))
            .expect("readme.md should be restored");
        assert_eq!(content, "# Hello");
    }

    #[test]
    fn import_empty_agent_file() {
        let store = test_store();
        let dir = tempfile::tempdir().expect("create temp dir");
        let agent = AgentFile {
            version: 1,
            exported_at: "2026-03-05T12:00:00Z".to_owned(),
            generator: "test".to_owned(),
            nous: NousInfo {
                id: "empty".to_owned(),
                name: None,
                model: None,
                config: serde_json::json!({}),
            },
            workspace: WorkspaceData {
                files: HashMap::new(),
                binary_files: vec![],
            },
            sessions: vec![],
            memory: None,
            knowledge: None,
        };

        let id_gen = counter_id_gen();
        let result = import_agent(
            &agent,
            &store,
            dir.path(),
            &*id_gen,
            &ImportOptions::default(),
        )
        .expect("import_agent with empty file should succeed");

        assert_eq!(result.files_restored, 0);
        assert_eq!(result.sessions_imported, 0);
    }

    #[test]
    fn import_corrupt_json_errors() {
        let garbage_inputs = [
            "",
            "not json",
            "{",
            "null",
            "42",
            r#"{"version": "not_a_number"}"#,
            r#"{"version": 1}"#, // missing required fields
            r#"{"version": 1, "exportedAt": "x", "generator": "x"}"#, // missing nous
        ];

        for input in &garbage_inputs {
            let result = serde_json::from_str::<AgentFile>(input);
            assert!(
                result.is_err(),
                "expected error for input: {input:?}, got: {result:?}"
            );
        }
    }

    #[test]
    fn import_missing_optional_sections() {
        let json = r#"{
            "version": 1,
            "exportedAt": "2026-03-05T12:00:00Z",
            "generator": "test",
            "nous": {
                "id": "sparse",
                "config": {}
            },
            "workspace": {
                "files": {},
                "binaryFiles": []
            },
            "sessions": []
        }"#;

        let agent: AgentFile = serde_json::from_str(json).expect("parse minimal agent file JSON");
        assert!(agent.nous.name.is_none());
        assert!(agent.nous.model.is_none());
        assert!(agent.memory.is_none());

        let store = test_store();
        let dir = tempfile::tempdir().expect("create temp dir");
        let id_gen = counter_id_gen();
        let result = import_agent(
            &agent,
            &store,
            dir.path(),
            &*id_gen,
            &ImportOptions::default(),
        )
        .expect("import_agent with sparse file should succeed");

        assert_eq!(result.sessions_imported, 0);
        assert_eq!(result.files_restored, 0);
    }

    #[test]
    fn export_import_preserves_timestamps() {
        let store = test_store();
        store
            .create_session("ses-ts", "ts-agent", "main", None, None)
            .expect("create session ses-ts");
        store
            .append_message("ses-ts", Role::User, "hello", None, None, 50)
            .expect("append user message");

        let dir = tempfile::tempdir().expect("create temp dir");
        let exported = export_agent(
            "ts-agent",
            None,
            None,
            serde_json::json!({}),
            &store,
            dir.path(),
            &ExportOptions::default(),
        )
        .expect("export_agent should succeed");

        let orig_created = exported.sessions[0].created_at.clone();
        let orig_updated = exported.sessions[0].updated_at.clone();
        let orig_msg_ts = exported.sessions[0].messages[0].created_at.clone();

        let json = serde_json::to_string(&exported).expect("serialize exported agent");
        let restored: AgentFile = serde_json::from_str(&json).expect("deserialize agent file");

        let import_store = test_store();
        let import_dir = tempfile::tempdir().expect("create import temp dir");
        let id_gen = counter_id_gen();
        import_agent(
            &restored,
            &import_store,
            import_dir.path(),
            &*id_gen,
            &ImportOptions::default(),
        )
        .expect("import_agent should succeed");

        let sessions = import_store
            .list_sessions(Some("ts-agent"))
            .expect("list_sessions should succeed");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].created_at, orig_created);
        assert_eq!(sessions[0].updated_at, orig_updated);

        let messages = import_store
            .get_history(&sessions[0].id, None)
            .expect("get_history should succeed");
        assert_eq!(messages[0].created_at, orig_msg_ts);
    }

    #[test]
    fn export_import_preserves_unicode() {
        let store = test_store();
        store
            .create_session("ses-uni", "uni", "main", None, None)
            .expect("create session ses-uni");

        let emoji = "Hello 🌍🔥 world";
        let cjk = "你好世界 こんにちは";
        let rtl = "مرحبا بالعالم";
        let combined = format!("{emoji} {cjk} {rtl}");

        store
            .append_message("ses-uni", Role::User, &combined, None, None, 200)
            .expect("append unicode message");
        store
            .add_note("ses-uni", "uni", "context", &combined)
            .expect("add unicode note");

        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::write(dir.path().join("unicode.txt"), &combined).expect("write unicode.txt");

        let exported = export_agent(
            "uni",
            Some("Ünïcödé"),
            None,
            serde_json::json!({}),
            &store,
            dir.path(),
            &ExportOptions::default(),
        )
        .expect("export_agent should succeed");

        let json =
            serde_json::to_string_pretty(&exported).expect("serialize exported agent to JSON");
        let restored: AgentFile =
            serde_json::from_str(&json).expect("deserialize agent file from JSON");

        let import_store = test_store();
        let import_dir = tempfile::tempdir().expect("create import temp dir");
        let id_gen = counter_id_gen();
        import_agent(
            &restored,
            &import_store,
            import_dir.path(),
            &*id_gen,
            &ImportOptions::default(),
        )
        .expect("import_agent should succeed");

        let content = std::fs::read_to_string(import_dir.path().join("unicode.txt"))
            .expect("unicode.txt should be restored");
        assert_eq!(content, combined);

        let sessions = import_store
            .list_sessions(Some("uni"))
            .expect("list_sessions should succeed");
        let messages = import_store
            .get_history(&sessions[0].id, None)
            .expect("get_history should succeed");
        assert_eq!(messages[0].content, combined);
    }

    #[test]
    fn export_import_large_data() {
        let store = test_store();
        for i in 0..100 {
            let sid = format!("ses-{i}");
            store
                .create_session(&sid, "bulk", &format!("key-{i}"), None, None)
                .expect("create bulk session");
            for j in 0..10 {
                store
                    .append_message(
                        &sid,
                        Role::User,
                        &format!("message {j} in session {i}"),
                        None,
                        None,
                        20,
                    )
                    .expect("append bulk message");
            }
        }

        let dir = tempfile::tempdir().expect("create temp dir");
        let exported = export_agent(
            "bulk",
            None,
            None,
            serde_json::json!({}),
            &store,
            dir.path(),
            &ExportOptions {
                max_messages_per_session: 0,
                include_archived: false,
            },
        )
        .expect("export_agent should succeed");

        assert_eq!(exported.sessions.len(), 100);
        let total_msgs: usize = exported.sessions.iter().map(|s| s.messages.len()).sum();
        assert_eq!(total_msgs, 1000);

        let json = serde_json::to_string(&exported).expect("serialize large export");
        let restored: AgentFile =
            serde_json::from_str(&json).expect("deserialize large agent file");

        let import_store = test_store();
        let import_dir = tempfile::tempdir().expect("create import temp dir");
        let id_gen = counter_id_gen();
        let result = import_agent(
            &restored,
            &import_store,
            import_dir.path(),
            &*id_gen,
            &ImportOptions::default(),
        )
        .expect("import_agent large data should succeed");

        assert_eq!(result.sessions_imported, 100);
        assert_eq!(result.messages_imported, 1000);
    }

    #[test]
    fn category_validation_uses_shared_constant() {
        let store = test_store();
        let dir = tempfile::tempdir().expect("create temp dir");

        let valid_categories = crate::schema::VALID_CATEGORIES;

        let mut agent = minimal_agent_file();
        agent.sessions[0].notes.clear();
        for cat in valid_categories {
            agent.sessions[0].notes.push(ExportedNote {
                category: (*cat).to_owned(),
                content: format!("note for {cat}"),
                created_at: "2026-03-05T10:30:00Z".to_owned(),
            });
        }
        agent.sessions[0].notes.push(ExportedNote {
            category: "bogus_category".to_owned(),
            content: "should default to context".to_owned(),
            created_at: "2026-03-05T10:30:00Z".to_owned(),
        });

        let id_gen = counter_id_gen();
        let result = import_agent(
            &agent,
            &store,
            dir.path(),
            &*id_gen,
            &ImportOptions::default(),
        )
        .expect("import_agent with all valid categories should succeed");

        assert_eq!(
            result.notes_imported as usize,
            valid_categories.len() + 1,
            "all valid + 1 defaulted note imported"
        );
    }

    #[test]
    fn import_rejects_future_version() {
        let store = test_store();
        let dir = tempfile::tempdir().expect("create temp dir");
        let mut agent = minimal_agent_file();
        agent.version = AGENT_FILE_VERSION + 1;

        let id_gen = counter_id_gen();
        let result = import_agent(
            &agent,
            &store,
            dir.path(),
            &*id_gen,
            &ImportOptions::default(),
        );

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains(&format!("{}", AGENT_FILE_VERSION + 1)),
            "error should mention the unsupported version number"
        );
    }

    #[test]
    fn import_preserves_note_content() {
        let store = test_store();
        let dir = tempfile::tempdir().expect("create temp dir");
        let mut agent = minimal_agent_file();
        agent.sessions[0].notes = vec![
            ExportedNote {
                category: "task".to_owned(),
                content: "first note content".to_owned(),
                created_at: "2026-03-05T10:30:00Z".to_owned(),
            },
            ExportedNote {
                category: "decision".to_owned(),
                content: "second note content".to_owned(),
                created_at: "2026-03-05T10:31:00Z".to_owned(),
            },
        ];

        let id_gen = counter_id_gen();
        let result = import_agent(
            &agent,
            &store,
            dir.path(),
            &*id_gen,
            &ImportOptions::default(),
        )
        .expect("import_agent should succeed");

        assert_eq!(result.notes_imported, 2);

        let sessions = store
            .list_sessions(Some("alice"))
            .expect("list_sessions should succeed");
        let notes = store
            .get_notes(&sessions[0].id)
            .expect("get_notes should succeed");
        assert_eq!(notes.len(), 2);
        let contents: Vec<&str> = notes.iter().map(|n| n.content.as_str()).collect();
        assert!(contents.contains(&"first note content"));
        assert!(contents.contains(&"second note content"));
    }

    #[test]
    fn import_with_empty_facts() {
        let store = test_store();
        let dir = tempfile::tempdir().expect("create temp dir");
        let mut agent = minimal_agent_file();
        agent.knowledge = Some(crate::portability::KnowledgeExport {
            facts: vec![],
            entities: vec![],
            relationships: vec![],
        });

        let id_gen = counter_id_gen();
        let result = import_agent(
            &agent,
            &store,
            dir.path(),
            &*id_gen,
            &ImportOptions::default(),
        )
        .expect("import_agent with empty knowledge should succeed");

        assert_eq!(result.sessions_imported, 1);
        assert_eq!(result.messages_imported, 2);
    }

    #[test]
    fn import_multiple_sessions_counts_correctly() {
        let store = test_store();
        let dir = tempfile::tempdir().expect("create temp dir");
        let mut agent = minimal_agent_file();
        // Add a second session
        agent.sessions.push(ExportedSession {
            id: "ses-2".to_owned(),
            session_key: "secondary".to_owned(),
            status: "active".to_owned(),
            session_type: "primary".to_owned(),
            message_count: 1,
            token_count_estimate: 50,
            distillation_count: 0,
            created_at: "2026-03-05T12:00:00Z".to_owned(),
            updated_at: "2026-03-05T12:00:00Z".to_owned(),
            working_state: None,
            distillation_priming: None,
            notes: vec![],
            messages: vec![ExportedMessage {
                role: "user".to_owned(),
                content: "second session".to_owned(),
                seq: 1,
                token_estimate: 50,
                is_distilled: false,
                created_at: "2026-03-05T12:00:00Z".to_owned(),
            }],
        });

        let id_gen = counter_id_gen();
        let result = import_agent(
            &agent,
            &store,
            dir.path(),
            &*id_gen,
            &ImportOptions::default(),
        )
        .expect("import_agent with multiple sessions should succeed");

        assert_eq!(result.sessions_imported, 2);
        assert_eq!(result.messages_imported, 3);
    }

    #[test]
    fn import_notes_counted_in_result() {
        let store = test_store();
        let dir = tempfile::tempdir().expect("create temp dir");
        let agent = minimal_agent_file();

        let id_gen = counter_id_gen();
        let result = import_agent(
            &agent,
            &store,
            dir.path(),
            &*id_gen,
            &ImportOptions::default(),
        )
        .expect("import_agent should succeed");

        // minimal_agent_file has 1 note
        assert_eq!(result.notes_imported, 1);
    }

    #[test]
    fn validate_relative_path_rejects_windows_drive() {
        assert!(!validate_relative_path("C:\\windows\\system32"));
        assert!(!validate_relative_path("D:file.txt"));
    }

    #[test]
    fn validate_relative_path_rejects_protocol() {
        assert!(!validate_relative_path("file:///etc/passwd"));
        assert!(!validate_relative_path("https://evil.com/payload"));
    }

    #[test]
    fn validate_relative_path_accepts_nested_dirs() {
        assert!(validate_relative_path("a/b/c/d.txt"));
        assert!(validate_relative_path("memory/2026-03-09.md"));
        assert!(validate_relative_path("SOUL.md"));
    }

    #[test]
    fn import_skip_both_flags_imports_nothing_from_disk_or_sessions() {
        let store = test_store();
        let dir = tempfile::tempdir().expect("create temp dir");
        let agent = minimal_agent_file();

        let id_gen = counter_id_gen();
        let result = import_agent(
            &agent,
            &store,
            dir.path(),
            &*id_gen,
            &ImportOptions {
                skip_sessions: true,
                skip_workspace: true,
                ..ImportOptions::default()
            },
        )
        .expect("import_agent with both skip flags should succeed");

        assert_eq!(result.files_restored, 0);
        assert_eq!(result.sessions_imported, 0);
        assert_eq!(result.messages_imported, 0);
        assert_eq!(result.notes_imported, 0);
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn export_import_preserves_content(
                content in "[a-zA-Z0-9 ]{1,200}",
                note_text in "[a-zA-Z0-9 ]{1,100}",
            ) {
                let store = test_store();
                store
                    .create_session("ses-prop", "prop-agent", "main", None, None)
                    .expect("create proptest session");
                store
                    .append_message("ses-prop", Role::User, &content, None, None, 50)
                    .expect("append proptest message");
                store
                    .add_note("ses-prop", "prop-agent", "context", &note_text)
                    .expect("add proptest note");

                let dir = tempfile::tempdir().expect("create proptest temp dir");
                let exported = export_agent(
                    "prop-agent",
                    None,
                    None,
                    serde_json::json!({}),
                    &store,
                    dir.path(),
                    &ExportOptions::default(),
                )
                .expect("export_agent should succeed in proptest");

                let json = serde_json::to_string(&exported)
                    .expect("serialize proptest export");
                let restored: AgentFile =
                    serde_json::from_str(&json).expect("deserialize proptest agent file");

                let import_store = test_store();
                let import_dir = tempfile::tempdir().expect("create proptest import dir");
                let id_gen = counter_id_gen();
                import_agent(
                    &restored,
                    &import_store,
                    import_dir.path(),
                    &*id_gen,
                    &ImportOptions::default(),
                )
                .expect("import_agent should succeed in proptest");

                let sessions = import_store
                    .list_sessions(Some("prop-agent"))
                    .expect("list_sessions should succeed in proptest");
                let messages = import_store
                    .get_history(&sessions[0].id, None)
                    .expect("get_history should succeed in proptest");
                prop_assert_eq!(&messages[0].content, &content);
            }
        }
    }
}
