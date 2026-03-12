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
    let ws = scan_workspace(Path::new("/nonexistent/path"))
        .expect("scan missing workspace returns empty");
    assert!(ws.files.is_empty());
    assert!(ws.binary_files.is_empty());
}

#[test]
fn scan_classifies_files() {
    let dir = tempfile::tempdir().expect("create tempdir");
    std::fs::write(dir.path().join("notes.md"), "# Notes").expect("write notes.md");
    std::fs::write(dir.path().join("data.bin"), b"\x00binary\x00").expect("write data.bin");
    std::fs::create_dir(dir.path().join(".git")).expect("create .git dir");
    std::fs::write(dir.path().join(".git/HEAD"), "ref: refs/heads/main")
        .expect("write .git/HEAD");

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
    std::fs::write(dir.path().join("node_modules/package.json"), "{}")
        .expect("write package.json");
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
    store
        .add_note("ses-1", "alice", "task", "testing")
        .expect("add note");

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
    store
        .mark_messages_distilled("ses-1", &[1])
        .expect("mark messages distilled");

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
    let restored: crate::portability::AgentFile =
        serde_json::from_str(&json).expect("deserialize agent from JSON");
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
    let restored: crate::portability::AgentFile =
        serde_json::from_str(&json).expect("deserialize unicode agent from JSON");
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
    let restored: crate::portability::AgentFile =
        serde_json::from_str(&json).expect("deserialize agent from JSON");
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
