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
