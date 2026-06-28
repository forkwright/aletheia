use super::*;

#[test]
fn agent_display_name_uses_name_if_present() {
    let agent = Agent {
        id: "syn".into(),
        name: Some("Syn".to_string()),
        model: None,
        emoji: None,
    };
    assert_eq!(agent.display_name(), "Syn");
}

#[test]
fn agent_display_name_falls_back_to_id() {
    let agent = Agent {
        id: "syn".into(),
        name: None,
        model: None,
        emoji: None,
    };
    assert_eq!(agent.display_name(), "syn");
}

#[test]
fn agent_display_name_empty_string_uses_empty() {
    let agent = Agent {
        id: "syn".into(),
        name: Some(String::new()),
        model: None,
        emoji: None,
    };
    // Empty string is still Some, so display_name returns it
    assert_eq!(agent.display_name(), "");
}

#[test]
fn agent_deserialization_minimal() {
    let json = r#"{"id": "syn"}"#;
    let agent: Agent = serde_json::from_str(json).unwrap();
    assert!(agent.id == *"syn");
    assert!(agent.name.is_none());
    assert!(agent.model.is_none());
    assert!(agent.emoji.is_none());
}

#[test]
fn agent_deserialization_full() {
    let json = r#"{"id": "syn", "name": "Syn", "model": "claude-opus-4-6", "emoji": "🧠"}"#;
    let agent: Agent = serde_json::from_str(json).unwrap();
    assert_eq!(agent.display_name(), "Syn");
    assert_eq!(agent.model.as_deref(), Some("claude-opus-4-6"));
}

#[test]
fn session_deserialization() {
    let json = r#"{
        "id": "sess-1",
        "nous_id": "syn",
        "session_key": "main",
        "message_count": 5,
        "status": "active"
    }"#;
    let session: Session = serde_json::from_str(json).unwrap();
    assert!(session.id == *"sess-1");
    assert!(session.nous_id == *"syn");
    assert_eq!(session.key, "main");
    assert_eq!(session.message_count, 5);
    assert_eq!(session.status.as_deref(), Some("active"));
}

#[test]
fn session_deserialization_defaults() {
    let json = r#"{"id": "s1", "nous_id": "n1", "session_key": "k1"}"#;
    let session: Session = serde_json::from_str(json).unwrap();
    assert_eq!(session.message_count, 0);
    assert!(session.status.is_none());
    assert!(session.session_type.is_none());
    assert!(session.updated_at.is_none());
}

#[test]
fn history_message_deserialization() {
    let json = r#"{
        "role": "user",
        "content": "hello",
        "created_at": "2025-01-01T00:00:00Z"
    }"#;
    let msg: HistoryMessage = serde_json::from_str(json).unwrap();
    assert!(msg.id.is_none());
    assert!(msg.seq.is_none());
    assert_eq!(msg.role, "user");
    assert!(msg.content.is_some());
    assert!(msg.created_at.is_some());
}

#[test]
fn history_message_deserializes_sequence_cursor() {
    let json = r#"{
        "id": 7,
        "seq": 42,
        "role": "assistant",
        "content": "hello",
        "created_at": "2025-01-01T00:00:00Z"
    }"#;
    let msg: HistoryMessage = serde_json::from_str(json).unwrap();
    assert_eq!(msg.id, Some(7));
    assert_eq!(msg.seq, Some(42));
}

#[test]
fn turn_outcome_deserialization() {
    let json = r#"{
        "text": "response",
        "nousId": "syn",
        "sessionId": "s1",
        "model": "claude-opus-4-6",
        "toolCalls": 3,
        "inputTokens": 100,
        "outputTokens": 50
    }"#;
    let outcome: TurnOutcome = serde_json::from_str(json).unwrap();
    assert_eq!(outcome.text, "response");
    assert_eq!(outcome.model.as_deref(), Some("claude-opus-4-6"));
    assert_eq!(outcome.tool_calls, 3);
    assert_eq!(outcome.input_tokens, 100);
}

#[test]
fn turn_outcome_defaults() {
    let json = r#"{
        "text": "r",
        "nousId": "n",
        "sessionId": "s",
        "model": "m"
    }"#;
    let outcome: TurnOutcome = serde_json::from_str(json).unwrap();
    assert_eq!(outcome.tool_calls, 0);
    assert_eq!(outcome.input_tokens, 0);
    assert_eq!(outcome.cache_read_tokens, 0);
}

// WHY: the gateway serializes an unresolved model as JSON null; a required
// field here failed deserialization and dropped the terminal turn event.
#[test]
fn turn_outcome_model_null_or_missing() {
    let null_model = r#"{"text": "r", "nousId": "n", "sessionId": "s", "model": null}"#;
    let outcome: TurnOutcome = serde_json::from_str(null_model).unwrap();
    assert!(outcome.model.is_none());

    let missing_model = r#"{"text": "r", "nousId": "n", "sessionId": "s"}"#;
    let outcome: TurnOutcome = serde_json::from_str(missing_model).unwrap();
    assert!(outcome.model.is_none());
}

#[test]
fn plan_step_deserialization() {
    let json = r#"{"id": 1, "label": "Step 1", "role": "analyst", "status": "pending"}"#;
    let step: PlanStep = serde_json::from_str(json).unwrap();
    assert_eq!(step.id, 1);
    assert_eq!(step.label, "Step 1");
    assert!(step.parallel.is_none());
}

#[test]
fn agents_response_accepts_both_keys() {
    let json_nous = r#"{"nous": [{"id": "a1"}]}"#;
    let resp: AgentsResponse = serde_json::from_str(json_nous).unwrap();
    assert_eq!(resp.nous.len(), 1);

    let json_agents = r#"{"agents": [{"id": "a1"}]}"#;
    let resp: AgentsResponse = serde_json::from_str(json_agents).unwrap();
    assert_eq!(resp.nous.len(), 1);
}

#[test]
fn login_response_debug_redacts_token() {
    let lr = LoginResponse {
        token: SecretString::from("secret-token-value"),
    };
    let debug = format!("{lr:?}");
    assert!(!debug.contains("secret-token-value"));
    assert!(debug.contains("REDACTED"));
}

#[test]
fn auth_mode_deserialization() {
    let json = r#"{"mode": "token"}"#;
    let mode: AuthMode = serde_json::from_str(json).unwrap();
    assert_eq!(mode.mode, "token");
}

#[test]
fn daily_entry_deserialization() {
    let json = r#"{"date": "2025-01-01", "cost": 1.50, "tokens": 1000, "turns": 5}"#;
    let entry: DailyEntry = serde_json::from_str(json).unwrap();
    assert_eq!(entry.date, "2025-01-01");
    assert!((entry.cost - 1.50).abs() < f64::EPSILON);
}

fn make_session(key: &str) -> Session {
    Session {
        id: "s1".into(),
        nous_id: "syn".into(),
        key: key.to_string(),
        status: None,
        message_count: 0,
        session_type: None,
        updated_at: None,
        display_name: None,
    }
}

#[test]
fn session_label_uses_display_name_when_set() {
    let mut s = make_session("main");
    s.display_name = Some("My Chat".to_string());
    assert_eq!(s.label(), "My Chat");
}

#[test]
fn session_label_falls_back_to_key() {
    let s = make_session("debug-session");
    assert_eq!(s.label(), "debug-session");
}

#[test]
fn session_label_ignores_empty_display_name() {
    let mut s = make_session("main");
    s.display_name = Some(String::new());
    assert_eq!(s.label(), "main");
}

#[test]
fn session_is_archived_by_status() {
    let mut s = make_session("main");
    assert!(!s.is_archived());
    s.status = Some("archived".to_string());
    assert!(s.is_archived());
}

#[test]
fn session_is_archived_by_key_pattern() {
    let s = make_session("foo:archived:bar");
    assert!(s.is_archived());
}

#[test]
fn session_is_interactive_normal() {
    let s = make_session("main");
    assert!(s.is_interactive());
}

#[test]
fn session_is_not_interactive_background() {
    let mut s = make_session("bg");
    s.session_type = Some("background".to_string());
    assert!(!s.is_interactive());
}

#[test]
fn session_is_not_interactive_cron() {
    let s = make_session("cron:daily");
    assert!(!s.is_interactive());
}

#[test]
fn session_is_not_interactive_prosoche() {
    let s = make_session("prosoche-wake");
    assert!(!s.is_interactive());
}

#[test]
fn session_is_not_interactive_agent_prefix() {
    let s = make_session("agent:sub-task");
    assert!(!s.is_interactive());
}

#[test]
fn session_is_not_interactive_daemon_prefix() {
    let s = make_session("daemon:prosoche");
    assert!(!s.is_interactive());
}

#[test]
fn session_deserialization_with_display_name() {
    let json = r#"{
        "id": "s1",
        "nous_id": "syn",
        "session_key": "main",
        "display_name": "My Chat"
    }"#;
    let session: Session = serde_json::from_str(json).unwrap();
    assert_eq!(session.display_name.as_deref(), Some("My Chat"));
    assert_eq!(session.label(), "My Chat");
}

#[test]
fn session_deserialization_with_name_alias() {
    let json = r#"{
        "id": "s1",
        "nous_id": "syn",
        "session_key": "main",
        "name": "Via Name"
    }"#;
    let session: Session = serde_json::from_str(json).unwrap();
    assert_eq!(session.display_name.as_deref(), Some("Via Name"));
    assert_eq!(session.label(), "Via Name");
}

#[test]
fn nous_tool_deserialization() {
    let json = r#"{"name": "read_file", "enabled": true}"#;
    let tool: NousTool = serde_json::from_str(json).unwrap();
    assert_eq!(tool.name, "read_file");
    assert!(tool.enabled);
}

#[test]
fn nous_tool_enabled_defaults_to_true() {
    let json = r#"{"name": "bash"}"#;
    let tool: NousTool = serde_json::from_str(json).unwrap();
    assert!(tool.enabled);
    assert!(!tool.metadata_verified);
}

#[test]
fn nous_tool_deserializes_risk_metadata() {
    let json = r#"{
        "name": "read_but_irreversible",
        "enabled": true,
        "description": "Misleading name",
        "category": "communication",
        "reversibility": "irreversible",
        "approval": "mandatory",
        "requires_approval": true,
        "destructive": true,
        "groups": ["mcp"],
        "source_plane": "organon_builtin",
        "policy_state": "callable",
        "metadata_verified": true,
        "auto_activate": true
    }"#;
    let tool: NousTool = serde_json::from_str(json).unwrap();
    assert_eq!(tool.category.as_deref(), Some("communication"));
    assert_eq!(tool.reversibility.as_deref(), Some("irreversible"));
    assert_eq!(tool.approval.as_deref(), Some("mandatory"));
    assert!(tool.requires_approval);
    assert!(tool.destructive);
    assert_eq!(tool.groups, vec!["mcp"]);
    assert_eq!(tool.source_plane.as_deref(), Some("organon_builtin"));
    assert_eq!(tool.policy_state.as_deref(), Some("callable"));
    assert!(tool.metadata_verified);
}

#[test]
fn nous_tools_response_deserialization() {
    let json = r#"{"tools": [{"name": "read_file", "enabled": true}, {"name": "bash", "enabled": false}]}"#;
    let resp: NousToolsResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.tools.len(), 2);
    assert!(resp.tools[0].enabled);
    assert!(!resp.tools[1].enabled);
}

#[test]
fn list_sessions_request_serializes_only_set_fields() {
    let params = ListSessionsRequest {
        nous_id: Some("syn".to_string()),
        search: None,
        status: Some("active".to_string()),
        limit: Some(25),
        after: None,
    };
    let json = serde_json::to_string(&params).unwrap();
    assert!(json.contains("\"nous_id\":\"syn\""));
    assert!(json.contains("\"status\":\"active\""));
    assert!(json.contains("\"limit\":25"));
    assert!(!json.contains("\"search\""));
    assert!(!json.contains("\"after\""));
}

#[test]
fn paginated_sessions_response_deserialization() {
    let json = r#"{
        "sessions": [{"id": "s1", "nous_id": "syn", "session_key": "main"}],
        "has_more": true,
        "next_cursor": "c2",
        "total": 42
    }"#;
    let resp: PaginatedSessionsResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.items.len(), 1);
    assert!(resp.has_more);
    assert_eq!(resp.next_cursor.as_deref(), Some("c2"));
    assert_eq!(resp.total, Some(42));
}

#[test]
fn paginated_sessions_response_accepts_items_alias() {
    let json = r#"{
        "items": [{"id": "s2", "nous_id": "syn", "session_key": "main"}],
        "has_more": false
    }"#;
    let resp: PaginatedSessionsResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.items.len(), 1);
    assert!(!resp.has_more);
    assert!(resp.next_cursor.is_none());
}
