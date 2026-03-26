//! Tests for import validation, dry-run, knowledge import, and error handling.
#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::as_conversions,
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
    assert!(
        agent.nous.name.is_none(),
        "nous name should be absent when not provided"
    );
    assert!(
        agent.nous.model.is_none(),
        "nous model should be absent when not provided"
    );
    assert!(
        agent.memory.is_none(),
        "memory section should be absent when not provided"
    );

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

    assert_eq!(
        result.sessions_imported, 0,
        "no sessions should be imported from sparse file"
    );
    assert_eq!(
        result.files_restored, 0,
        "no files should be restored from empty workspace"
    );
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
    assert_eq!(sessions.len(), 1, "one session should be imported");
    assert_eq!(
        sessions[0].created_at, orig_created,
        "session created_at should be preserved"
    );
    assert_eq!(
        sessions[0].updated_at, orig_updated,
        "session updated_at should be preserved"
    );

    let messages = import_store
        .get_history(&sessions[0].id, None)
        .expect("get_history should succeed");
    assert_eq!(
        messages[0].created_at, orig_msg_ts,
        "message timestamp should be preserved"
    );
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

    let json = serde_json::to_string_pretty(&exported).expect("serialize exported agent to JSON");
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
    assert_eq!(
        content, combined,
        "unicode file content should survive export/import roundtrip"
    );

    let sessions = import_store
        .list_sessions(Some("uni"))
        .expect("list_sessions should succeed");
    let messages = import_store
        .get_history(&sessions[0].id, None)
        .expect("get_history should succeed");
    assert_eq!(
        messages[0].content, combined,
        "unicode message content should survive export/import roundtrip"
    );
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

    assert_eq!(
        exported.sessions.len(),
        100,
        "all 100 sessions should be exported"
    );
    let total_msgs: usize = exported.sessions.iter().map(|s| s.messages.len()).sum();
    assert_eq!(total_msgs, 1000, "all 1000 messages should be exported");

    let json = serde_json::to_string(&exported).expect("serialize large export");
    let restored: AgentFile = serde_json::from_str(&json).expect("deserialize large agent file");

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

    assert_eq!(
        result.sessions_imported, 100,
        "all 100 sessions should be imported"
    );
    assert_eq!(
        result.messages_imported, 1000,
        "all 1000 messages should be imported"
    );
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

    assert!(result.is_err(), "future version should be rejected");
    let err = result
        .expect_err("future version should be rejected")
        .to_string();
    let future_version = AGENT_FILE_VERSION + 1;
    assert!(
        err.contains(&format!("{future_version}")),
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

    assert_eq!(result.notes_imported, 2, "both notes should be imported");

    let sessions = store
        .list_sessions(Some("alice"))
        .expect("list_sessions should succeed");
    let notes = store
        .get_notes(&sessions[0].id)
        .expect("get_notes should succeed");
    assert_eq!(notes.len(), 2, "two notes should be stored");
    let contents: Vec<&str> = notes.iter().map(|n| n.content.as_str()).collect();
    assert!(
        contents.contains(&"first note content"),
        "first note content should be preserved"
    );
    assert!(
        contents.contains(&"second note content"),
        "second note content should be preserved"
    );
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

    assert_eq!(
        result.sessions_imported, 1,
        "session should still be imported with empty knowledge"
    );
    assert_eq!(
        result.messages_imported, 2,
        "messages should still be imported with empty knowledge"
    );
}

#[test]
fn import_multiple_sessions_counts_correctly() {
    let store = test_store();
    let dir = tempfile::tempdir().expect("create temp dir");
    let mut agent = minimal_agent_file();
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

    assert_eq!(
        result.sessions_imported, 2,
        "both sessions should be imported"
    );
    assert_eq!(
        result.messages_imported, 3,
        "all messages across both sessions should be imported"
    );
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

    assert_eq!(
        result.notes_imported, 1,
        "one note should be imported from minimal agent file"
    );
}

#[test]
fn validate_relative_path_rejects_windows_drive() {
    assert!(
        !validate_relative_path("C:\\windows\\system32"),
        "windows absolute path should be rejected"
    );
    assert!(
        !validate_relative_path("D:file.txt"),
        "windows drive-relative path should be rejected"
    );
}

#[test]
fn validate_relative_path_rejects_protocol() {
    assert!(
        !validate_relative_path("file:///etc/passwd"),
        "file protocol path should be rejected"
    );
    assert!(
        !validate_relative_path("https://evil.com/payload"),
        "url protocol path should be rejected"
    );
}

#[test]
fn validate_relative_path_accepts_nested_dirs() {
    assert!(
        validate_relative_path("a/b/c/d.txt"),
        "nested relative path should be accepted"
    );
    assert!(
        validate_relative_path("memory/2026-03-09.md"),
        "date-named file in subdirectory should be accepted"
    );
    assert!(
        validate_relative_path("SOUL.md"),
        "uppercase filename should be accepted"
    );
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

    assert_eq!(
        result.files_restored, 0,
        "no files should be restored when skip_workspace is set"
    );
    assert_eq!(
        result.sessions_imported, 0,
        "no sessions should be imported when skip_sessions is set"
    );
    assert_eq!(
        result.messages_imported, 0,
        "no messages should be imported when skip_sessions is set"
    );
    assert_eq!(
        result.notes_imported, 0,
        "no notes should be imported when skip_sessions is set"
    );
}

mod proptests {
    use proptest::prelude::*;

    use super::*;

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
