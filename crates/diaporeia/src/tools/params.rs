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
#[cfg_attr(not(test), expect(dead_code, reason = "fields consumed by JsonSchema/Deserialize derives, not accessed directly"))]
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct KnowledgeSearchParams {
    /// The search query text.
    pub query: String,
    /// Optional nous agent ID to scope the search.
    pub nous_id: Option<String>,
    /// Maximum number of results to return.
    pub limit: Option<u32>,
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
        let json = r#"{"nous_id": "chiron", "session_key": "debug-session"}"#;
        let params: SessionCreateParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.nous_id, "chiron");
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
        let json = r#"{"query": "session recall", "nous_id": "chiron", "limit": 10}"#;
        let params: KnowledgeSearchParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.query, "session recall");
        assert_eq!(params.nous_id.as_deref(), Some("chiron"));
        assert_eq!(params.limit, Some(10));
    }

    #[test]
    fn session_list_params_allows_empty_filter() {
        let json = r"{}";
        let params: SessionListParams = serde_json::from_str(json).unwrap();
        assert!(params.nous_id.is_none());
    }
}
