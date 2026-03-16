//! Agent portability schema: AgentFile format for cross-runtime export/import.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Agent file format version.
pub const AGENT_FILE_VERSION: u32 = 1;

/// Portable agent file: wire-compatible with the TypeScript `AgentFile` format.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
}

/// Agent identity and configuration snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NousInfo {
    pub id: String,
    pub name: Option<String>,
    pub model: Option<String>,
    pub config: serde_json::Value,
}

/// Workspace file snapshot: text content included, binary paths listed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceData {
    pub files: HashMap<String, String>,
    pub binary_files: Vec<String>,
}

/// Session snapshot with full message history and metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedSession {
    pub id: String,
    pub session_key: String,
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
}

/// Single message within an exported session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedMessage {
    pub role: String,
    pub content: String,
    pub seq: i64,
    pub token_estimate: i64,
    pub is_distilled: bool,
    pub created_at: String,
}

/// Agent note that survives distillation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedNote {
    pub category: String,
    pub content: String,
    pub created_at: String,
}

/// Optional memory data (vectors and/or knowledge graph).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryData {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vectors: Option<Vec<ExportedVector>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph: Option<GraphData>,
}

/// Memory vector entry (embeddings omitted: regenerated on import).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedVector {
    pub id: String,
    pub text: String,
    pub metadata: serde_json::Value,
}

/// Knowledge graph snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
                    },
                    ExportedMessage {
                        role: "assistant".to_owned(),
                        content: "hi there".to_owned(),
                        seq: 2,
                        token_estimate: 100,
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

        // Top-level keys
        assert!(value.get("exportedAt").is_some(), "missing exportedAt");
        assert!(value.get("exported_at").is_none(), "snake_case leaked");

        // Workspace keys
        let ws = value.get("workspace").expect("workspace key must exist");
        assert!(ws.get("binaryFiles").is_some(), "missing binaryFiles");
        assert!(ws.get("binary_files").is_none(), "snake_case leaked");

        // Session keys
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

        // Message keys
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
        assert_eq!(AGENT_FILE_VERSION, 1);
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
}
