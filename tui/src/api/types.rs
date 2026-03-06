use serde::{Deserialize, Serialize};

use crate::id::{NousId, PlanId, SessionId, TurnId};

// --- Agent ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: NousId,
    pub name: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub emoji: Option<String>,
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
    pub agents: Vec<Agent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionsResponse {
    pub sessions: Vec<Session>,
}
