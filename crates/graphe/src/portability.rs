//! Agent portability schema: AgentFile format for cross-runtime export/import.
#![cfg_attr(
    test,
    expect(
        clippy::indexing_slicing,
        reason = "tests index into serde_json::Value arrays of known length from the fixture"
    )
)]

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Agent file format version.
///
/// - **v1** (pre-#4163): silently lossy — distilled messages dropped from
///   exports, `working_state`/`memory`/`knowledge` never serialized,
///   `status`/`created_at`/metrics reset on import.
/// - **v2** (#4163): faithful round-trip. Producers populate every populated
///   slot from live stores; consumers preserve session status, timestamps,
///   metrics, and per-message `created_at`/`is_distilled` on import.
/// - **v3** (#4590): binary workspace files include base64 contents, byte
///   count, and sha256 so import restores bytes instead of path-only entries.
///
/// The version bump declares the fidelity contract: consumers MUST reject
/// older versions (or pipe them through a migration) so they cannot silently
/// drop fields that the current version expects to round-trip.
pub const AGENT_FILE_VERSION: u32 = 3;

/// Machine-readable metadata describing the completeness of an export.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportMetadata {
    /// Whether the export contains every populated slot the format supports.
    #[serde(default)]
    pub lossless: bool,
    /// Sections that were omitted because they were excluded or unavailable.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub omitted_sections: Vec<OmittedSection>,
    /// Slots where the exported data was truncated by operator request.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub truncations: Vec<TruncationRecord>,
    /// Workspace symbolic-link traversal policy used by the exporter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_symlink_policy: Option<String>,
}

/// A section that was omitted from an export.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OmittedSection {
    /// Section name, e.g. "knowledge" or "sessions".
    pub section: String,
    /// Human/machine-readable reason, e.g. "`store_unavailable`".
    pub reason: String,
    /// Number of omitted items, when meaningful.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
}

/// A truncation applied to an export slot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TruncationRecord {
    /// Section name, e.g. "`session_messages`".
    pub section: String,
    /// Identifier of the truncated item, when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_id: Option<String>,
    /// Maximum number of items that were exported.
    pub limit: usize,
    /// Original number of items, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original: Option<usize>,
}

/// Portable agent file: wire-compatible with the TypeScript `AgentFile` format.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    missing_docs,
    reason = "portability struct fields are self-documenting by name"
)]
pub struct AgentFile {
    pub version: u32,
    pub exported_at: String,
    pub generator: String,
    pub nous: NousInfo,
    pub workspace: WorkspaceData,
    pub sessions: Vec<ExportedSession>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryData>,
    /// Knowledge graph export (facts, entities, relationships).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub knowledge: Option<KnowledgeExport>,
    /// Export completeness metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub export_metadata: Option<ExportMetadata>,
}

/// Agent identity and configuration snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    missing_docs,
    reason = "portability struct fields are self-documenting by name"
)]
pub struct NousInfo {
    pub id: String, // kanon:ignore RUST/primitive-for-domain-id — wire-format serde type; newtype would break JSON compatibility and change public API
    pub name: Option<String>,
    pub model: Option<String>,
    pub config: serde_json::Value,
}

/// Workspace file snapshot: text content included, binary bytes encoded.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    missing_docs,
    reason = "portability struct fields are self-documenting by name"
)]
pub struct WorkspaceData {
    pub files: HashMap<String, String>,
    pub binary_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub binary_file_contents: Vec<BinaryFileData>,
}

/// Binary workspace file payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    missing_docs,
    reason = "portability struct fields are self-documenting by name"
)]
pub struct BinaryFileData {
    pub path: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub contents_base64: String,
}

/// Session snapshot with full message history and metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    missing_docs,
    reason = "portability struct fields are self-documenting by name"
)]
pub struct ExportedSession {
    pub id: String, // kanon:ignore RUST/primitive-for-domain-id — wire-format serde type; newtype would break JSON compatibility and change public API
    pub session_key: String, // kanon:ignore RUST/plain-string-secret - NOTE: lookup slug, not a secret credential
    pub status: String,
    pub session_type: String,
    pub message_count: i64,
    pub token_count_estimate: i64,
    pub distillation_count: i64,
    pub created_at: String,
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_state: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distillation_priming: Option<serde_json::Value>,
    pub notes: Vec<ExportedNote>,
    pub messages: Vec<ExportedMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_records: Option<Vec<ExportedUsageRecord>>,
    // NOTE: artefact_meta from Session is intentionally excluded — store-internal
    // provenance, not user/agent data. The identity fields below were excluded from
    // v1; they now round-trip, guarded by serde(default) so older files still load.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub parent_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub thread_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub transport: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub last_input_tokens: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub bootstrap_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub last_distilled_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub computed_context_tokens: Option<i64>,
}

/// Single message within an exported session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    missing_docs,
    reason = "portability struct fields are self-documenting by name"
)]
pub struct ExportedMessage {
    pub role: String,
    pub content: String,
    pub seq: i64,
    pub token_estimate: i64,
    pub is_distilled: bool,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
}

/// Agent note that survives distillation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    missing_docs,
    reason = "portability struct fields are self-documenting by name"
)]
pub struct ExportedNote {
    pub category: String,
    pub content: String,
    pub created_at: String,
}

/// Durable token usage record for a single turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    missing_docs,
    reason = "portability struct fields are self-documenting by name"
)]
pub struct ExportedUsageRecord {
    pub turn_seq: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// Optional memory data (vectors and/or knowledge graph).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    missing_docs,
    reason = "portability struct fields are self-documenting by name"
)]
pub struct MemoryData {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vectors: Option<Vec<ExportedVector>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph: Option<GraphData>,
}

/// Memory vector entry (embeddings omitted: regenerated on import).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    missing_docs,
    reason = "portability struct fields are self-documenting by name"
)]
pub struct ExportedVector {
    pub id: String, // kanon:ignore RUST/primitive-for-domain-id — wire-format serde type; newtype would break JSON compatibility and change public API
    pub text: String,
    pub metadata: serde_json::Value,
}

/// Knowledge graph snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    missing_docs,
    reason = "portability struct fields are self-documenting by name"
)]
pub struct GraphData {
    pub nodes: Vec<serde_json::Value>,
    pub edges: Vec<serde_json::Value>,
}

/// Knowledge graph export for backup, migration, and debugging.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeExport {
    /// All facts from the knowledge graph.
    pub facts: Vec<crate::knowledge::Fact>,
    /// All entities from the knowledge graph.
    pub entities: Vec<crate::knowledge::Entity>,
    /// All relationships between entities.
    pub relationships: Vec<crate::knowledge::Relationship>,
    /// Exact fact-to-entity links that should be restored on import.
    #[serde(default)]
    pub fact_entity_edges: Vec<FactEntityEdge>,
}

/// A single fact-to-entity link from the knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    missing_docs,
    reason = "portability struct fields are self-documenting by name"
)]
pub struct FactEntityEdge {
    pub fact_id: crate::id::FactId,
    pub entity_id: crate::id::EntityId,
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn sample_agent_file() -> AgentFile {
        AgentFile {
            version: AGENT_FILE_VERSION,
            exported_at: "2026-03-05T12:00:00Z".to_owned(),
            generator: "aletheia-rust/0.10.0".to_owned(),
            nous: NousInfo {
                id: "syn".to_owned(),
                name: Some("Syn".to_owned()),
                model: Some("claude-sonnet-4-6".to_owned()),
                config: serde_json::json!({"domains": ["general"]}),
            },
            workspace: WorkspaceData {
                files: HashMap::from([
                    ("memory/notes.md".to_owned(), "# Notes\n".to_owned()),
                    ("config.yaml".to_owned(), "key: value\n".to_owned()),
                ]),
                binary_files: vec!["avatar.png".to_owned()],
                binary_file_contents: vec![BinaryFileData {
                    path: "avatar.png".to_owned(),
                    size_bytes: 4,
                    sha256: "0f4636c78f65d3639ece5a064b5ae753e3408614a14fb18ab4d7540d2c248543"
                        .to_owned(),
                    contents_base64: "iVBORw==".to_owned(),
                }],
            },
            sessions: vec![ExportedSession {
                id: "ses-001".to_owned(),
                session_key: "main".to_owned(),
                status: "active".to_owned(),
                session_type: "primary".to_owned(),
                message_count: 2,
                token_count_estimate: 150,
                distillation_count: 0,
                created_at: "2026-03-05T10:00:00Z".to_owned(),
                updated_at: "2026-03-05T11:00:00Z".to_owned(),
                working_state: None,
                distillation_priming: None,
                notes: vec![ExportedNote {
                    category: "task".to_owned(),
                    content: "working on portability".to_owned(),
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
                        tool_call_id: None,
                        tool_name: None,
                    },
                    ExportedMessage {
                        role: "tool_result".to_owned(),
                        content: "tool output".to_owned(),
                        seq: 2,
                        token_estimate: 15,
                        is_distilled: false,
                        created_at: "2026-03-05T10:00:01Z".to_owned(),
                        tool_call_id: Some("call-1".to_owned()),
                        tool_name: Some("read_file".to_owned()),
                    },
                ],
                usage_records: Some(vec![ExportedUsageRecord {
                    turn_seq: 1,
                    input_tokens: 65,
                    output_tokens: 100,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                    model: Some("claude-sonnet-4-6".to_owned()),
                }]),
                parent_session_id: Some("ses-parent".to_owned()),
                thread_id: Some("thread-9".to_owned()),
                transport: Some("stdio".to_owned()),
                display_name: Some("Main Session".to_owned()),
                last_input_tokens: Some(64),
                bootstrap_hash: Some("abc123".to_owned()),
                last_distilled_at: Some("2026-03-05T10:45:00Z".to_owned()),
                computed_context_tokens: Some(210),
            }],
            memory: None,
            knowledge: None,
            export_metadata: None,
        }
    }

    #[test]
    fn serde_roundtrip() {
        let original = sample_agent_file();
        let json = serde_json::to_string_pretty(&original).expect("AgentFile is serializable");
        let restored: AgentFile = serde_json::from_str(&json).expect("round-trip JSON is valid");

        assert_eq!(restored.version, original.version);
        assert_eq!(restored.exported_at, original.exported_at);
        assert_eq!(restored.generator, original.generator);
        assert_eq!(restored.nous.id, original.nous.id);
        assert_eq!(restored.workspace.files.len(), 2);
        assert_eq!(restored.workspace.binary_files.len(), 1);
        assert_eq!(restored.workspace.binary_file_contents.len(), 1);
        assert_eq!(restored.sessions.len(), 1);
        assert_eq!(restored.sessions[0].messages.len(), 2);
        assert_eq!(restored.sessions[0].notes.len(), 1);
        assert!(restored.memory.is_none());
    }

    #[test]
    fn camel_case_json_keys() {
        let agent = sample_agent_file();
        let value: serde_json::Value =
            serde_json::to_value(&agent).expect("AgentFile is serializable");

        assert!(value.get("exportedAt").is_some(), "missing exportedAt");
        assert!(value.get("exported_at").is_none(), "snake_case leaked");

        let ws = value.get("workspace").expect("workspace key must exist");
        assert!(ws.get("binaryFiles").is_some(), "missing binaryFiles");
        assert!(
            ws.get("binaryFileContents").is_some(),
            "missing binaryFileContents"
        );
        assert!(ws.get("binary_files").is_none(), "snake_case leaked");

        let session = &value["sessions"][0];
        assert!(session.get("sessionKey").is_some(), "missing sessionKey");
        assert!(session.get("sessionType").is_some(), "missing sessionType");
        assert!(
            session.get("messageCount").is_some(),
            "missing messageCount"
        );
        assert!(
            session.get("tokenCountEstimate").is_some(),
            "missing tokenCountEstimate"
        );
        assert!(
            session.get("distillationCount").is_some(),
            "missing distillationCount"
        );
        assert!(session.get("createdAt").is_some(), "missing createdAt");
        assert!(session.get("updatedAt").is_some(), "missing updatedAt");

        let msg = &session["messages"][0];
        assert!(msg.get("tokenEstimate").is_some(), "missing tokenEstimate");
        assert!(msg.get("isDistilled").is_some(), "missing isDistilled");
        assert!(msg.get("createdAt").is_some(), "missing createdAt");
    }

    #[test]
    fn memory_omitted_when_none() {
        let agent = sample_agent_file();
        let json = serde_json::to_string(&agent).expect("AgentFile is serializable");
        assert!(
            !json.contains("\"memory\""),
            "memory should be omitted when None"
        );
    }

    #[test]
    fn agent_file_serde_roundtrip() {
        let original = sample_agent_file();
        let json = serde_json::to_string(&original).expect("AgentFile is serializable");
        let restored: AgentFile = serde_json::from_str(&json).expect("round-trip JSON is valid");
        assert_eq!(restored.version, original.version);
        assert_eq!(restored.exported_at, original.exported_at);
        assert_eq!(restored.generator, original.generator);
        assert_eq!(restored.nous.id, original.nous.id);
        assert_eq!(restored.sessions.len(), original.sessions.len());
        assert_eq!(
            restored.sessions[0].messages.len(),
            original.sessions[0].messages.len()
        );
    }

    #[test]
    fn agent_file_empty_sessions() {
        let mut agent = sample_agent_file();
        agent.sessions = vec![];
        let json = serde_json::to_string(&agent).expect("AgentFile is serializable");
        let back: AgentFile = serde_json::from_str(&json).expect("round-trip JSON is valid");
        assert!(back.sessions.is_empty());
    }

    #[test]
    fn agent_file_optional_fields_omitted() {
        let agent = sample_agent_file();
        let json = serde_json::to_string(&agent).expect("AgentFile is serializable");
        assert!(
            !json.contains("\"memory\""),
            "memory=None should be omitted"
        );
        assert!(
            !json.contains("\"knowledge\""),
            "knowledge=None should be omitted"
        );
    }

    #[test]
    fn format_version_constant() {
        assert_eq!(AGENT_FILE_VERSION, 3);
    }

    #[test]
    fn memory_included_when_present() {
        let mut agent = sample_agent_file();
        agent.memory = Some(MemoryData {
            vectors: Some(vec![ExportedVector {
                id: "vec-1".to_owned(),
                text: "important fact".to_owned(),
                metadata: serde_json::json!({}),
            }]),
            graph: None,
        });
        let value: serde_json::Value =
            serde_json::to_value(&agent).expect("AgentFile is serializable");
        let mem = value.get("memory").expect("memory key must exist when set");
        assert!(mem.get("vectors").is_some());
        assert!(
            mem.get("graph").is_none(),
            "graph should be omitted when None"
        );
    }

    #[test]
    fn message_preserves_tool_call_id_and_tool_name() {
        let original = sample_agent_file();
        let json = serde_json::to_string(&original).expect("AgentFile is serializable");
        let restored: AgentFile = serde_json::from_str(&json).expect("round-trip JSON is valid");
        let messages = &restored.sessions[0].messages;
        assert_eq!(messages[0].tool_call_id, None);
        assert_eq!(messages[0].tool_name, None);
        assert_eq!(
            messages[1].tool_call_id.as_deref(),
            Some("call-1"),
            "tool_call_id must round-trip"
        );
        assert_eq!(
            messages[1].tool_name.as_deref(),
            Some("read_file"),
            "tool_name must round-trip"
        );
    }

    #[test]
    fn session_preserves_usage_records() {
        let original = sample_agent_file();
        let json = serde_json::to_string(&original).expect("AgentFile is serializable");
        let restored: AgentFile = serde_json::from_str(&json).expect("round-trip JSON is valid");
        let records = restored.sessions[0]
            .usage_records
            .as_ref()
            .expect("usage_records should be present");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].turn_seq, 1);
        assert_eq!(records[0].input_tokens, 65);
        assert_eq!(records[0].output_tokens, 100);
        assert_eq!(records[0].model.as_deref(), Some("claude-sonnet-4-6"));
    }

    #[test]
    fn session_identity_fields_round_trip() {
        let original = sample_agent_file();
        let json = serde_json::to_string(&original).expect("AgentFile is serializable");
        let restored: AgentFile = serde_json::from_str(&json).expect("round-trip JSON is valid");
        let s = &restored.sessions[0];
        assert_eq!(s.parent_session_id.as_deref(), Some("ses-parent"));
        assert_eq!(s.thread_id.as_deref(), Some("thread-9"));
        assert_eq!(s.transport.as_deref(), Some("stdio"));
        assert_eq!(s.display_name.as_deref(), Some("Main Session"));
        assert_eq!(s.last_input_tokens, Some(64));
        assert_eq!(s.bootstrap_hash.as_deref(), Some("abc123"));
        assert_eq!(s.last_distilled_at.as_deref(), Some("2026-03-05T10:45:00Z"));
        assert_eq!(s.computed_context_tokens, Some(210));
    }

    #[test]
    fn session_identity_fields_omitted_when_none() {
        let mut agent = sample_agent_file();
        let s = &mut agent.sessions[0];
        s.parent_session_id = None;
        s.thread_id = None;
        s.transport = None;
        s.display_name = None;
        s.last_input_tokens = None;
        s.bootstrap_hash = None;
        s.last_distilled_at = None;
        s.computed_context_tokens = None;
        let json = serde_json::to_string(&agent).expect("AgentFile is serializable");
        assert!(
            !json.contains("parentSessionId"),
            "None parent_session_id leaked"
        );
        assert!(!json.contains("threadId"), "None thread_id leaked");
        assert!(
            !json.contains("bootstrapHash"),
            "None bootstrap_hash leaked"
        );
    }

    #[test]
    fn session_identity_fields_deserialize_when_absent() {
        let json = r#"{
            "id":"ses-x","sessionKey":"main","status":"active","sessionType":"primary",
            "messageCount":0,"tokenCountEstimate":0,"distillationCount":0,
            "createdAt":"2026-03-05T10:00:00Z","updatedAt":"2026-03-05T11:00:00Z",
            "notes":[],"messages":[]
        }"#;
        let restored: ExportedSession =
            serde_json::from_str(json).expect("legacy session JSON is valid");
        assert_eq!(restored.parent_session_id, None);
        assert_eq!(restored.computed_context_tokens, None);
    }

    #[test]
    fn knowledge_export_round_trips_fact_entity_edges() {
        let original = KnowledgeExport {
            facts: vec![],
            entities: vec![],
            relationships: vec![],
            fact_entity_edges: vec![FactEntityEdge {
                fact_id: crate::id::FactId::new("fact-1").expect("valid fact id"),
                entity_id: crate::id::EntityId::new("entity-1").expect("valid entity id"),
            }],
        };
        let json = serde_json::to_string(&original).expect("KnowledgeExport is serializable");
        let restored: KnowledgeExport =
            serde_json::from_str(&json).expect("round-trip JSON is valid");
        assert_eq!(restored.fact_entity_edges.len(), 1);
        assert_eq!(restored.fact_entity_edges[0].fact_id.as_str(), "fact-1");
        assert_eq!(restored.fact_entity_edges[0].entity_id.as_str(), "entity-1");
    }

    #[test]
    fn knowledge_export_deserializes_without_fact_entity_edges() {
        let json = r#"{"facts":[],"entities":[],"relationships":[]}"#;
        let restored: KnowledgeExport = serde_json::from_str(json).expect("legacy JSON is valid");
        assert!(restored.fact_entity_edges.is_empty());
    }
}
