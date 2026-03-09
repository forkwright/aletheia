use serde::{Deserialize, Serialize};

use crate::id::{NousId, PlanId, SessionId, TurnId};

// --- Agent ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: NousId,
    /// Display name — falls back to `id` if absent.
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub emoji: Option<String>,
}

impl Agent {
    /// Display name: uses `name` if set, otherwise `id`.
    #[must_use]
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }
}

// --- Session ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: SessionId,
    #[serde(rename = "nousId")]
    pub nous_id: NousId,
    #[serde(rename = "sessionKey")]
    pub key: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(rename = "messageCount", default)]
    pub message_count: u32,
    #[serde(rename = "sessionType", default)]
    pub session_type: Option<String>,
    #[serde(rename = "updatedAt", default)]
    pub updated_at: Option<String>,
}

// --- History ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryMessage {
    pub role: String,
    #[serde(default)]
    pub content: Option<serde_json::Value>,
    #[serde(rename = "createdAt", default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(rename = "toolName", default)]
    pub tool_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryResponse {
    pub messages: Vec<HistoryMessage>,
}

// --- Turn outcome ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnOutcome {
    pub text: String,
    #[serde(rename = "nousId")]
    pub nous_id: NousId,
    #[serde(rename = "sessionId")]
    pub session_id: SessionId,
    pub model: String,
    #[serde(rename = "toolCalls", default)]
    pub tool_calls: u32,
    #[serde(rename = "inputTokens", default)]
    pub input_tokens: u32,
    #[serde(rename = "outputTokens", default)]
    pub output_tokens: u32,
    #[serde(rename = "cacheReadTokens", default)]
    pub cache_read_tokens: u32,
    #[serde(rename = "cacheWriteTokens", default)]
    pub cache_write_tokens: u32,
    #[serde(default)]
    pub error: Option<String>,
}

// --- Plans ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub id: u32,
    pub label: String,
    pub role: String,
    #[serde(default)]
    pub parallel: Option<Vec<u32>>,
    pub status: String,
    #[serde(default)]
    pub result: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub id: PlanId,
    #[serde(rename = "sessionId")]
    pub session_id: SessionId,
    #[serde(rename = "nousId")]
    pub nous_id: NousId,
    pub steps: Vec<PlanStep>,
    #[serde(rename = "totalEstimatedCostCents", default)]
    pub total_estimated_cost_cents: u32,
    pub status: String,
}

// --- SSE events ---

#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum SseEvent {
    Connected,
    Disconnected,
    Init {
        active_turns: Vec<ActiveTurn>,
    },
    TurnBefore {
        nous_id: NousId,
        session_id: SessionId,
        turn_id: TurnId,
    },
    TurnAfter {
        nous_id: NousId,
        session_id: SessionId,
    },
    ToolCalled {
        nous_id: NousId,
        tool_name: String,
    },
    ToolFailed {
        nous_id: NousId,
        tool_name: String,
        error: String,
    },
    StatusUpdate {
        nous_id: NousId,
        status: String,
    },
    SessionCreated {
        nous_id: NousId,
        session_id: SessionId,
    },
    SessionArchived {
        nous_id: NousId,
        session_id: SessionId,
    },
    DistillBefore {
        nous_id: NousId,
    },
    DistillStage {
        nous_id: NousId,
        stage: String,
    },
    DistillAfter {
        nous_id: NousId,
    },
    Ping,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveTurn {
    #[serde(rename = "nousId")]
    pub nous_id: NousId,
    #[serde(rename = "sessionId")]
    pub session_id: SessionId,
    #[serde(rename = "turnId")]
    pub turn_id: TurnId,
}

// --- Auth ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthMode {
    pub mode: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct LoginResponse {
    pub token: String,
}

impl std::fmt::Debug for LoginResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoginResponse")
            .field("token", &"[REDACTED]")
            .finish()
    }
}

// --- Costs ---

#[expect(dead_code, reason = "deserialization target for /api/v1/costs")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostSummary {
    #[serde(rename = "totalCost", default)]
    pub total_cost: f64,
    #[serde(default)]
    pub agents: Vec<AgentCost>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCost {
    #[serde(rename = "agentId")]
    pub agent_id: NousId,
    #[serde(rename = "totalCost", default)]
    pub total_cost: f64,
    #[serde(default)]
    pub turns: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyResponse {
    pub daily: Vec<DailyEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyEntry {
    pub date: String,
    pub cost: f64,
    #[serde(default)]
    pub tokens: u64,
    #[serde(default)]
    pub turns: u32,
}

// --- Response wrappers ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentsResponse {
    /// Server returns `{"nous": [...]}` — accept both keys for resilience.
    #[serde(alias = "agents")]
    pub nous: Vec<Agent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionsResponse {
    pub sessions: Vec<Session>,
}

#[cfg(test)]
mod tests {
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
        let json = r#"{"id": "syn", "name": "Syn", "model": "claude-opus-4-6", "emoji": "\ud83e\udde0"}"#;
        let agent: Agent = serde_json::from_str(json).unwrap();
        assert_eq!(agent.display_name(), "Syn");
        assert_eq!(agent.model.as_deref(), Some("claude-opus-4-6"));
    }

    #[test]
    fn session_deserialization() {
        let json = r#"{
            "id": "sess-1",
            "nousId": "syn",
            "sessionKey": "main",
            "messageCount": 5,
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
        let json = r#"{"id": "s1", "nousId": "n1", "sessionKey": "k1"}"#;
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
            "createdAt": "2025-01-01T00:00:00Z"
        }"#;
        let msg: HistoryMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.role, "user");
        assert!(msg.content.is_some());
        assert!(msg.created_at.is_some());
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
            token: "secret-token-value".to_string(),
        };
        let debug = format!("{:?}", lr);
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
}
