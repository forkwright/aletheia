//! Cross-format compatibility test: verifies Rust can parse TS-exported `AgentFile`.
#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test: vec indices are valid after asserting len"
)]

use aletheia_mneme::portability::AgentFile;

#[test]
fn deserialize_ts_format_agent_file() {
    let json = include_str!("fixtures/ts-compat-agent.json");
    let agent: AgentFile = serde_json::from_str(json).expect("failed to parse TS agent file");

    assert_eq!(agent.version, 1);
    assert_eq!(agent.generator, "aletheia-export/1.0");
    assert_eq!(agent.nous.id, "syn");
    assert_eq!(agent.nous.name.as_deref(), Some("Syn"));
    assert_eq!(agent.nous.model.as_deref(), Some("claude-sonnet-4-6"));

    assert_eq!(agent.workspace.files.len(), 2);
    assert!(agent.workspace.files.contains_key("memory/2026-03-05.md"));
    assert_eq!(agent.workspace.binary_files.len(), 2);

    assert_eq!(agent.sessions.len(), 1);
    let session = &agent.sessions[0];
    assert_eq!(session.session_key, "main");
    assert_eq!(session.status, "active");
    assert_eq!(session.session_type, "primary");
    assert_eq!(session.message_count, 3);
    assert_eq!(session.distillation_count, 1);
    assert!(session.working_state.is_some());
    assert!(session.distillation_priming.is_some());
    assert_eq!(session.notes.len(), 2);
    assert_eq!(session.messages.len(), 3);
    assert!(session.messages[0].is_distilled);
    assert!(!session.messages[2].is_distilled);

    let memory = agent.memory.as_ref().expect("memory should be present");
    let vectors = memory.vectors.as_ref().expect("vectors should be present");
    assert_eq!(vectors.len(), 1);
    assert_eq!(vectors[0].id, "vec-001");
    let graph = memory.graph.as_ref().expect("graph should be present");
    assert_eq!(graph.nodes.len(), 2);
    assert_eq!(graph.edges.len(), 1);
}

#[test]
fn roundtrip_preserves_ts_format() {
    let json = include_str!("fixtures/ts-compat-agent.json");
    let agent: AgentFile = serde_json::from_str(json).unwrap();
    let re_serialized = serde_json::to_string_pretty(&agent).unwrap();
    let re_parsed: AgentFile = serde_json::from_str(&re_serialized).unwrap();

    assert_eq!(agent.version, re_parsed.version);
    assert_eq!(agent.nous.id, re_parsed.nous.id);
    assert_eq!(agent.sessions.len(), re_parsed.sessions.len());
    assert_eq!(
        agent.sessions[0].messages.len(),
        re_parsed.sessions[0].messages.len()
    );
}
