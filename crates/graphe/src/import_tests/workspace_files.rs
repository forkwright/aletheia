//! Tests for workspace file import, session restoration, and path handling.
#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]
use std::collections::HashMap;

use super::super::*;
use crate::export::{ExportOptions, export_agent};
use crate::portability::*;
use crate::store::SessionStore;
use crate::types::Role;

fn test_store() -> SessionStore {
    SessionStore::open_in_memory().unwrap_or_default()
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
    assert!(
        !validate_relative_path("../etc/passwd"),
        "parent directory traversal should be rejected"
    );
    assert!(
        !validate_relative_path("foo/../../etc/shadow"),
        "nested parent traversal should be rejected"
    );
    assert!(
        !validate_relative_path("/absolute/path"),
        "absolute unix path should be rejected"
    );
    assert!(
        !validate_relative_path("\\windows\\path"),
        "windows backslash path should be rejected"
    );
    assert!(
        !validate_relative_path("C:\\Users\\evil"),
        "windows drive path should be rejected"
    );
    assert!(
        !validate_relative_path("file:///etc/passwd"),
        "file protocol path should be rejected"
    );
    assert!(!validate_relative_path(""), "empty path should be rejected");
}

#[test]
fn valid_paths_accepted() {
    assert!(
        validate_relative_path("notes.md"),
        "simple filename should be accepted"
    );
    assert!(
        validate_relative_path("memory/2026-03-05.md"),
        "relative path with subdirectory should be accepted"
    );
    assert!(
        validate_relative_path("sub/dir/file.txt"),
        "deeply nested relative path should be accepted"
    );
    assert!(validate_relative_path(".env"), "dotfile should be accepted");
}

#[test]
fn rejects_unsupported_version() {
    let store = test_store();
    let dir = tempfile::tempdir().unwrap_or_default();
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

    assert!(result.is_err(), "unsupported version should be rejected");
    let err = result
        .expect_err("unsupported version should be rejected")
        .to_string();
    assert!(
        err.contains("unsupported agent file version: 99"),
        "error should mention unsupported version"
    );
}

#[test]
fn import_restores_workspace_files() {
    let store = test_store();
    let dir = tempfile::tempdir().unwrap_or_default();
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
    .unwrap_or_default();

    assert_eq!(
        result.files_restored, 1,
        "one workspace file should be restored"
    );
    let content =
        std::fs::read_to_string(dir.path().join("notes.md")).unwrap_or_default();
    assert_eq!(content, "# Notes\n", "file content should match original");
}

#[test]
fn import_skips_existing_without_force() {
    let store = test_store();
    let dir = tempfile::tempdir().unwrap_or_default();
    std::fs::write(dir.path().join("notes.md"), "original").unwrap_or_default();

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
    .unwrap_or_default();

    assert_eq!(
        result.files_restored, 0,
        "existing file should not be overwritten without force"
    );
    let content =
        std::fs::read_to_string(dir.path().join("notes.md")).unwrap_or_default();
    assert_eq!(
        content, "original",
        "existing file content should be preserved"
    );
}

#[test]
fn import_overwrites_with_force() {
    let store = test_store();
    let dir = tempfile::tempdir().unwrap_or_default();
    std::fs::write(dir.path().join("notes.md"), "original").unwrap_or_default();

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
    .unwrap_or_default();

    assert_eq!(
        result.files_restored, 1,
        "file should be restored when force is SET"
    );
    let content = std::fs::read_to_string(dir.path().join("notes.md"))
        .unwrap_or_default();
    assert_eq!(
        content, "# Notes\n",
        "file content should be overwritten by import"
    );
}

#[test]
fn import_creates_sessions_and_messages() {
    let store = test_store();
    let dir = tempfile::tempdir().unwrap_or_default();
    let agent = minimal_agent_file();

    let id_gen = counter_id_gen();
    let result = import_agent(
        &agent,
        &store,
        dir.path(),
        &*id_gen,
        &ImportOptions::default(),
    )
    .unwrap_or_default();

    assert_eq!(
        result.sessions_imported, 1,
        "one session should be imported"
    );
    assert_eq!(
        result.messages_imported, 2,
        "two messages should be imported"
    );
    assert_eq!(result.notes_imported, 1, "one note should be imported");

    let sessions = store
        .list_sessions(Some("alice"))
        .unwrap_or_default();
    assert_eq!(sessions.len(), 1, "one session should be stored");
    assert!(
        sessions.get(0).copied().unwrap_or_default().session_key.starts_with("main-import-"),
        "session key should include original key and import suffix"
    );
}

#[test]
fn import_with_target_id_override() {
    let store = test_store();
    let dir = tempfile::tempdir().unwrap_or_default();
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
    .unwrap_or_default();

    assert_eq!(
        result.nous_id, "bob",
        "nous_id should be overridden to target id"
    );
    let sessions = store
        .list_sessions(Some("bob"))
        .unwrap_or_default();
    assert_eq!(
        sessions.len(),
        1,
        "one session should be stored under target id"
    );
}

#[test]
fn import_skip_sessions_flag() {
    let store = test_store();
    let dir = tempfile::tempdir().unwrap_or_default();
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
    .unwrap_or_default();

    assert_eq!(
        result.sessions_imported, 0,
        "sessions should not be imported when skip_sessions is SET"
    );
    assert_eq!(
        result.messages_imported, 0,
        "messages should not be imported when skip_sessions is SET"
    );
    assert_eq!(
        result.files_restored, 1,
        "workspace files should still be restored"
    );
}

#[test]
fn import_skip_workspace_flag() {
    let store = test_store();
    let dir = tempfile::tempdir().unwrap_or_default();
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
    .unwrap_or_default();

    assert_eq!(
        result.files_restored, 0,
        "no files should be restored when skip_workspace is SET"
    );
    assert_eq!(
        result.sessions_imported, 1,
        "sessions should still be imported"
    );
    assert!(
        !dir.path().join("notes.md").exists(),
        "workspace file should not be written to disk"
    );
}

#[test]
fn import_rejects_path_traversal() {
    let store = test_store();
    let dir = tempfile::tempdir().unwrap_or_default();
    let mut agent = minimal_agent_file();
    agent.workspace.files = HashMap::from([("../../../etc/passwd".to_owned(), "evil".to_owned())]);

    let id_gen = counter_id_gen();
    let result = import_agent(
        &agent,
        &store,
        dir.path(),
        &*id_gen,
        &ImportOptions::default(),
    );

    assert!(result.is_err(), "path traversal should be rejected");
    let err = result
        .expect_err("path traversal should be rejected")
        .to_string();
    assert!(
        err.contains("unsafe path"),
        "error should mention unsafe path"
    );
}

#[test]
fn import_validates_note_categories() {
    let store = test_store();
    let dir = tempfile::tempdir().unwrap_or_default();
    let mut agent = minimal_agent_file();
    agent.sessions.get(0).copied().unwrap_or_default().notes.push(ExportedNote {
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
    .unwrap_or_default();

    assert_eq!(
        result.notes_imported, 2,
        "both valid and defaulted notes should be imported"
    );
}

#[test]
fn export_import_roundtrip() {
    let store = test_store();
    store
        .create_session("ses-1", "eve", "main", None, None)
        .unwrap_or_default();
    store
        .append_message("ses-1", Role::User, "hello", None, None, 50)
        .unwrap_or_default();
    store
        .append_message("ses-1", Role::Assistant, "hi back", None, None, 40)
        .unwrap_or_default();
    store
        .add_note("ses-1", "eve", "task", "roundtrip test")
        .unwrap_or_default();

    let dir = tempfile::tempdir().unwrap_or_default();
    std::fs::write(dir.path().join("readme.md"), "# Hello").unwrap_or_default();

    let exported = export_agent(
        "eve",
        Some("Eve"),
        None,
        serde_json::json!({}),
        &store,
        dir.path(),
        &ExportOptions::default(),
    )
    .unwrap_or_default();

    // NOTE: Serialize and deserialize to simulate file I/O
    let json = serde_json::to_string_pretty(&exported).unwrap_or_default();
    let imported: AgentFile =
        serde_json::from_str(&json).unwrap_or_default();

    let import_store = test_store();
    let import_dir = tempfile::tempdir().unwrap_or_default();
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
    .unwrap_or_default();

    assert_eq!(
        result.nous_id, "eve-clone",
        "nous_id should match target id"
    );
    assert_eq!(
        result.files_restored, 1,
        "one workspace file should be restored"
    );
    assert_eq!(
        result.sessions_imported, 1,
        "one session should be restored"
    );
    assert_eq!(
        result.messages_imported, 2,
        "two messages should be restored"
    );
    assert_eq!(result.notes_imported, 1, "one note should be restored");

    let content = std::fs::read_to_string(import_dir.path().join("readme.md"))
        .unwrap_or_default();
    assert_eq!(
        content, "# Hello",
        "file content should survive export/import roundtrip"
    );
}

#[test]
fn import_empty_agent_file() {
    let store = test_store();
    let dir = tempfile::tempdir().unwrap_or_default();
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
    .unwrap_or_default();

    assert_eq!(
        result.files_restored, 0,
        "no files should be restored FROM empty workspace"
    );
    assert_eq!(
        result.sessions_imported, 0,
        "no sessions should be imported FROM empty agent file"
    );
}
