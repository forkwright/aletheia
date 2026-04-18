//! Parameter structs for MCP tools.
//!
//! Each struct derives `schemars::JsonSchema` for automatic JSON Schema generation.

use schemars::JsonSchema;
use serde::Deserialize;

/// Parameters for creating a session.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct SessionCreateParams {
    /// The nous agent ID to create the session for.
    pub nous_id: String,
    /// Optional session key. If omitted, defaults to "main".
    pub session_key: Option<String>,
}

/// Parameters for listing sessions.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct SessionListParams {
    /// Filter by nous agent ID.
    pub nous_id: Option<String>,
}

/// Parameters for sending a message to a session.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct SessionMessageParams {
    /// The nous agent ID.
    pub nous_id: String,
    /// The session key identifying the conversation.
    pub session_key: String,
    /// The message content to send.
    pub content: String,
}

/// Parameters for retrieving session history.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct SessionHistoryParams {
    /// The session ID (the database primary key).
    pub session_id: String,
    /// Maximum number of messages to return.
    pub limit: Option<i64>,
}

/// Parameters for querying a specific nous agent.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct NousIdParam {
    /// The nous agent ID.
    pub nous_id: String,
}

/// Parameters for knowledge search.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "fields consumed by JsonSchema/Deserialize derives, not accessed directly"
    )
)]
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct KnowledgeSearchParams {
    /// The search query text.
    pub query: String,
    /// Optional nous agent ID to scope the search.
    pub nous_id: Option<String>,
    /// Maximum number of results to return.
    pub limit: Option<u32>,
}

/// Parameters for `knowledge.recall` — semantic + BM25 recall.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct KnowledgeRecallParams {
    /// The recall query text (used for both BM25 and vector search).
    pub query: String,
    /// Scope the recall to a specific nous agent ID.
    ///
    /// When omitted, results span all agents visible to the caller.
    pub nous_id: Option<String>,
    /// Maximum number of facts to return (default: 20).
    pub limit: Option<u32>,
}

/// Sensitivity classification for a new fact.
///
/// Accepted values: `"public"` (default), `"internal"`, `"confidential"`.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct KnowledgeInsertParams {
    /// The fact content to store.
    pub content: String,
    /// The nous agent ID that owns this fact.
    pub nous_id: String,
    /// Data-sovereignty sensitivity: `"public"`, `"internal"`, or `"confidential"`.
    ///
    /// Defaults to `"public"` when omitted.
    pub sensitivity: Option<String>,
}

/// Parameters for `knowledge.forget` — soft-delete a fact.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct KnowledgeForgetParams {
    /// The fact ID to soft-delete.
    pub fact_id: String,
    /// Human-readable reason for forgetting.
    ///
    /// Accepted values: `"user_requested"` (default), `"superseded"`,
    /// `"incorrect"`, `"privacy"`.
    pub reason: Option<String>,
}

/// Parameters for `knowledge.get` — fetch a single fact by ID.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct KnowledgeGetParams {
    /// The fact ID to retrieve.
    pub fact_id: String,
}

/// Parameters for `knowledge.graph_neighbors` — traverse entity edges.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct KnowledgeGraphNeighborsParams {
    /// The entity ID to start from.
    pub entity_id: String,
    /// Maximum number of hops from the start entity (default: 2, max: 4).
    pub depth: Option<u32>,
}

/// Parameters for `repomix.pack` — pack crate source into compressed context.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct RepomixPackParams {
    /// Crate names to pack (e.g. `["diaporeia"]` or `["nous", "pylon"]`).
    pub crate_names: Vec<String>,
    /// Template to use: `single_crate`, `crate_with_deps`, or `cross_crate`.
    pub template: String,
    /// Maximum output tokens (overrides config default if provided).
    pub max_tokens: Option<u32>,
}

/// Parameters for `repomix.template_get` — fetch a template definition.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct RepomixTemplateGetParams {
    /// Template name.
    pub name: String,
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn session_create_params_deserializes_with_defaults() {
        let json = r#"{"nous_id": "syn"}"#;
        let params: SessionCreateParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.nous_id, "syn");
        assert!(params.session_key.is_none());
    }

    #[test]
    fn session_create_params_deserializes_with_session_key() {
        let json = r#"{"nous_id": "analyst", "session_key": "debug-session"}"#;
        let params: SessionCreateParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.nous_id, "analyst");
        assert_eq!(params.session_key.as_deref(), Some("debug-session"));
    }

    #[test]
    fn session_message_params_requires_all_fields() {
        let json = r#"{"nous_id": "syn", "session_key": "main", "content": "hello agent"}"#;
        let params: SessionMessageParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.nous_id, "syn");
        assert_eq!(params.session_key, "main");
        assert_eq!(params.content, "hello agent");
    }

    #[test]
    fn session_message_params_rejects_missing_content() {
        let json = r#"{"nous_id": "syn", "session_key": "main"}"#;
        let result = serde_json::from_str::<SessionMessageParams>(json);
        assert!(
            result.is_err(),
            "missing required field 'content' must fail"
        );
    }

    #[test]
    fn session_history_params_deserializes_with_optional_limit() {
        let json = r#"{"session_id": "01HXYZ123", "limit": 50}"#;
        let params: SessionHistoryParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.session_id, "01HXYZ123");
        assert_eq!(params.limit, Some(50));

        let json_no_limit = r#"{"session_id": "01HXYZ123"}"#;
        let params2: SessionHistoryParams = serde_json::from_str(json_no_limit).unwrap();
        assert!(params2.limit.is_none());
    }

    #[test]
    fn nous_id_param_deserializes_from_json() {
        let json = r#"{"nous_id": "syn"}"#;
        let params: NousIdParam = serde_json::from_str(json).unwrap();
        assert_eq!(params.nous_id, "syn");
    }

    #[test]
    fn knowledge_search_params_deserializes_with_all_optional_fields() {
        let json = r#"{"query": "session recall", "nous_id": "analyst", "limit": 10}"#;
        let params: KnowledgeSearchParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.query, "session recall");
        assert_eq!(params.nous_id.as_deref(), Some("analyst"));
        assert_eq!(params.limit, Some(10));
    }

    #[test]
    fn session_list_params_allows_empty_filter() {
        let json = r"{}";
        let params: SessionListParams = serde_json::from_str(json).unwrap();
        assert!(params.nous_id.is_none());
    }

    #[test]
    fn knowledge_recall_params_deserializes_minimal() {
        let json = r#"{"query": "Rust ownership"}"#;
        let params: KnowledgeRecallParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.query, "Rust ownership");
        assert!(params.nous_id.is_none());
        assert!(params.limit.is_none());
    }

    #[test]
    fn knowledge_recall_params_deserializes_full() {
        let json = r#"{"query": "error handling", "nous_id": "syn", "limit": 10}"#;
        let params: KnowledgeRecallParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.query, "error handling");
        assert_eq!(params.nous_id.as_deref(), Some("syn"));
        assert_eq!(params.limit, Some(10));
    }

    #[test]
    fn knowledge_insert_params_requires_content_and_nous_id() {
        let json = r#"{"content": "Rust uses ownership for memory safety.", "nous_id": "syn"}"#;
        let params: KnowledgeInsertParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.nous_id, "syn");
        assert!(params.sensitivity.is_none());
    }

    #[test]
    fn knowledge_insert_params_rejects_missing_nous_id() {
        let json = r#"{"content": "some fact"}"#;
        let result = serde_json::from_str::<KnowledgeInsertParams>(json);
        assert!(result.is_err(), "missing nous_id must fail");
    }

    #[test]
    fn knowledge_forget_params_deserializes_with_defaults() {
        let json = r#"{"fact_id": "f-abc123"}"#;
        let params: KnowledgeForgetParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.fact_id, "f-abc123");
        assert!(params.reason.is_none());
    }

    #[test]
    fn knowledge_get_params_deserializes() {
        let json = r#"{"fact_id": "f-xyz"}"#;
        let params: KnowledgeGetParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.fact_id, "f-xyz");
    }

    #[test]
    fn knowledge_graph_neighbors_params_deserializes() {
        let json = r#"{"entity_id": "e-42", "depth": 3}"#;
        let params: KnowledgeGraphNeighborsParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.entity_id, "e-42");
        assert_eq!(params.depth, Some(3));
    }

    #[test]
    fn knowledge_graph_neighbors_params_allows_default_depth() {
        let json = r#"{"entity_id": "e-1"}"#;
        let params: KnowledgeGraphNeighborsParams = serde_json::from_str(json).unwrap();
        assert!(params.depth.is_none());
    }

    #[test]
    fn repomix_pack_params_deserializes() {
        let json =
            r#"{"crate_names": ["diaporeia"], "template": "single_crate", "max_tokens": 1000}"#;
        let params: RepomixPackParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.crate_names, vec!["diaporeia"]);
        assert_eq!(params.template, "single_crate");
        assert_eq!(params.max_tokens, Some(1000));
    }

    #[test]
    fn repomix_pack_params_rejects_missing_template() {
        let json = r#"{"crate_names": ["diaporeia"]}"#;
        let result = serde_json::from_str::<RepomixPackParams>(json);
        assert!(result.is_err(), "missing template must fail");
    }

    #[test]
    fn repomix_template_get_params_deserializes() {
        let json = r#"{"name": "single_crate"}"#;
        let params: RepomixTemplateGetParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.name, "single_crate");
    }
}
