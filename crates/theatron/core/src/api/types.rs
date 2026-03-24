//! Request and response types for the Aletheia REST API.

use serde::{Deserialize, Serialize};

use aletheia_koina::secret::SecretString;

use crate::id::{NousId, PlanId, SessionId, TurnId};

/// A registered agent (nous) in the system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    /// Agent identifier.
    pub id: NousId,
    /// Display name: falls back to `id` if absent.
    #[serde(default)]
    pub name: Option<String>,
    /// Model backing this agent.
    #[serde(default)]
    pub model: Option<String>,
    /// Emoji icon for the agent.
    #[serde(default)]
    pub emoji: Option<String>,
}

impl Agent {
    /// Display name: uses `name` if set, otherwise `id`.
    #[must_use]
    pub fn display_name(&self) -> &str {
        // kanon:ignore RUST/pub-visibility
        self.name.as_deref().unwrap_or(&self.id)
    }
}

/// A session within an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Session identifier.
    pub id: SessionId,
    /// Agent this session belongs to.
    pub nous_id: NousId,
    /// Session key (human-readable slug, not a secret).
    #[serde(rename = "session_key")]
    pub key: String, // kanon:ignore RUST/plain-string-secret
    /// Session status (e.g. "active", "archived").
    #[serde(default)]
    pub status: Option<String>,
    /// Number of messages in the session.
    #[serde(default)]
    pub message_count: u32,
    /// Session type (e.g. "background").
    #[serde(default)]
    pub session_type: Option<String>,
    /// Last-updated timestamp.
    #[serde(default)]
    pub updated_at: Option<String>,
    /// User-assigned display name.
    #[serde(default)]
    pub display_name: Option<String>,
}

impl Session {
    /// Label for display: prefers `display_name`, falls back to `key`.
    #[must_use]
    pub fn label(&self) -> &str {
        // kanon:ignore RUST/pub-visibility
        self.display_name
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(&self.key)
    }

    /// Whether this session has been archived.
    #[must_use]
    pub fn is_archived(&self) -> bool {
        // kanon:ignore RUST/pub-visibility
        self.status.as_deref() == Some("archived") || self.key.contains(":archived:")
    }

    /// Whether this session accepts interactive user input.
    #[must_use]
    pub fn is_interactive(&self) -> bool {
        // kanon:ignore RUST/pub-visibility
        !self.is_archived()
            && self.session_type.as_deref() != Some("background")
            && !self.key.starts_with("cron:")
            && !self.key.starts_with("daemon:")
            && !self.key.starts_with("prosoche")
            && !self.key.starts_with("agent:")
    }
}

/// A single message from session history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryMessage {
    /// Role: "user", "assistant", or "tool".
    pub role: String,
    /// Message content (text or structured).
    #[serde(default)]
    pub content: Option<serde_json::Value>,
    /// When the message was created.
    #[serde(default)]
    pub created_at: Option<String>,
    /// Model that generated this message (assistant messages only).
    #[serde(default)]
    pub model: Option<String>,
    /// Tool name if this is a tool-result message.
    #[serde(default)]
    pub tool_name: Option<String>,
}

/// Wrapper for the history endpoint response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryResponse {
    /// Messages in chronological order.
    pub messages: Vec<HistoryMessage>,
}

/// Summary of a completed turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnOutcome {
    /// Final text output.
    pub text: String,
    /// Agent that processed this turn.
    #[serde(rename = "nousId")]
    pub nous_id: NousId,
    /// Session this turn belongs to.
    #[serde(rename = "sessionId")]
    pub session_id: SessionId,
    /// Model used for this turn.
    pub model: String,
    /// Number of tool calls made.
    #[serde(rename = "toolCalls", default)]
    pub tool_calls: u32,
    /// Input tokens consumed.
    #[serde(rename = "inputTokens", default)]
    pub input_tokens: u32,
    /// Output tokens generated.
    #[serde(rename = "outputTokens", default)]
    pub output_tokens: u32,
    /// Tokens read from cache.
    #[serde(rename = "cacheReadTokens", default)]
    pub cache_read_tokens: u32,
    /// Tokens written to cache.
    #[serde(rename = "cacheWriteTokens", default)]
    pub cache_write_tokens: u32,
    /// Error message, if the turn errored.
    #[serde(default)]
    pub error: Option<String>,
}

/// A single step within a plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    /// Step index.
    pub id: u32,
    /// Human-readable label.
    pub label: String,
    /// Role responsible for this step.
    pub role: String,
    /// Steps that can run in parallel with this one.
    #[serde(default)]
    pub parallel: Option<Vec<u32>>,
    /// Current status of this step.
    pub status: String,
    /// Result summary after completion.
    #[serde(default)]
    pub result: Option<String>,
}

/// A multi-step execution plan proposed by the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    /// Plan identifier.
    pub id: PlanId,
    /// Session this plan was proposed in.
    #[serde(rename = "sessionId")]
    pub session_id: SessionId,
    /// Agent that proposed the plan.
    #[serde(rename = "nousId")]
    pub nous_id: NousId,
    /// Ordered list of plan steps.
    pub steps: Vec<PlanStep>,
    /// Estimated total cost in cents.
    #[serde(rename = "totalEstimatedCostCents", default)]
    pub total_estimated_cost_cents: u32,
    /// Plan status.
    pub status: String,
}

/// Application-level SSE events from `GET /api/v1/events`.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum SseEvent {
    /// SSE connection established.
    Connected,
    /// SSE connection lost (will auto-reconnect).
    Disconnected,
    /// Initial state dump with currently active turns.
    Init {
        /// Turns that are currently in progress.
        active_turns: Vec<ActiveTurn>,
    },
    /// A turn is about to start.
    TurnBefore {
        /// Agent processing the turn.
        nous_id: NousId,
        /// Session the turn belongs to.
        session_id: SessionId,
        /// Turn identifier.
        turn_id: TurnId,
    },
    /// A turn has completed.
    TurnAfter {
        /// Agent that processed the turn.
        nous_id: NousId,
        /// Session the turn belongs to.
        session_id: SessionId,
    },
    /// A tool was invoked during a turn.
    ToolCalled {
        /// Agent invoking the tool.
        nous_id: NousId,
        /// Name of the tool.
        tool_name: String,
    },
    /// A tool invocation failed.
    ToolFailed {
        /// Agent whose tool failed.
        nous_id: NousId,
        /// Name of the failed tool.
        tool_name: String,
        /// Error description.
        error: String,
    },
    /// Agent status changed.
    StatusUpdate {
        /// Agent whose status changed.
        nous_id: NousId,
        /// New status value.
        status: String,
    },
    /// A new session was created.
    SessionCreated {
        /// Agent the session was created for.
        nous_id: NousId,
        /// New session identifier.
        session_id: SessionId,
    },
    /// A session was archived.
    SessionArchived {
        /// Agent the session belongs to.
        nous_id: NousId,
        /// Archived session identifier.
        session_id: SessionId,
    },
    /// Memory distillation is about to start.
    DistillBefore {
        /// Agent undergoing distillation.
        nous_id: NousId,
    },
    /// Memory distillation progressed to a new stage.
    DistillStage {
        /// Agent undergoing distillation.
        nous_id: NousId,
        /// Current distillation stage.
        stage: String,
    },
    /// Memory distillation completed.
    DistillAfter {
        /// Agent that completed distillation.
        nous_id: NousId,
    },
    /// A new checkpoint was created in a planning project.
    CheckpointCreated {
        /// Project the checkpoint belongs to.
        project_id: String,
        /// Identifier of the created checkpoint.
        checkpoint_id: String,
    },
    /// A checkpoint's status changed (approved, skipped, overridden).
    CheckpointUpdated {
        /// Project the checkpoint belongs to.
        project_id: String,
        /// Identifier of the updated checkpoint.
        checkpoint_id: String,
        /// New status value (e.g. "approved", "skipped", "overridden").
        status: String,
    },
    /// Server heartbeat.
    Ping,
}

/// A turn currently in progress, reported in the `init` SSE event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveTurn {
    /// Agent processing this turn.
    #[serde(rename = "nousId")]
    pub nous_id: NousId,
    /// Session this turn belongs to.
    #[serde(rename = "sessionId")]
    pub session_id: SessionId,
    /// Turn identifier.
    #[serde(rename = "turnId")]
    pub turn_id: TurnId,
}

/// Server authentication mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthMode {
    /// Authentication mode (e.g. "token", "none").
    pub mode: String,
}

/// Response from the login endpoint.
#[derive(Clone, Serialize, Deserialize)]
pub struct LoginResponse {
    /// Authentication token.
    pub token: SecretString,
}

impl std::fmt::Debug for LoginResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoginResponse")
            .field("token", &self.token)
            .finish()
    }
}

/// Cost summary across agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostSummary {
    /// Total cost across all agents.
    #[serde(rename = "totalCost", default)]
    pub total_cost: f64,
    /// Per-agent cost breakdown.
    #[serde(default)]
    pub agents: Vec<AgentCost>,
}

/// Cost for a single agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCost {
    /// Agent identifier.
    #[serde(rename = "agentId")]
    pub agent_id: NousId,
    /// Total cost for this agent.
    #[serde(rename = "totalCost", default)]
    pub total_cost: f64,
    /// Number of turns processed.
    #[serde(default)]
    pub turns: u32,
}

/// Response from the daily costs endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyResponse {
    /// Daily cost entries.
    pub daily: Vec<DailyEntry>,
}

/// A single day's cost and usage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyEntry {
    /// Date string (YYYY-MM-DD).
    pub date: String,
    /// Cost in dollars.
    pub cost: f64,
    /// Total tokens consumed.
    #[serde(default)]
    pub tokens: u64,
    /// Number of turns.
    #[serde(default)]
    pub turns: u32,
}

/// Wrapper for the agents list endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentsResponse {
    /// Server returns `{"nous": [...]}`: accept both keys for resilience.
    #[serde(alias = "agents")]
    pub nous: Vec<Agent>,
}

/// Wrapper for the sessions list endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionsResponse {
    /// List of sessions.
    pub sessions: Vec<Session>,
}

/// A tool available to an agent, with its enablement state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NousTool {
    /// Tool name.
    pub name: String,
    /// Whether the tool is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// Wrapper for the tools list endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NousToolsResponse {
    /// List of tools.
    pub tools: Vec<NousTool>,
}

// ---------------------------------------------------------------------------
// Planning verification types
// ---------------------------------------------------------------------------

/// Verification status for a single requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum VerificationStatus {
    /// Requirement fully demonstrated.
    Verified,
    /// Some but not all criteria demonstrated.
    PartiallyVerified,
    /// No verification evidence found.
    Unverified,
    /// Verification attempted but explicitly failed.
    Failed,
}

/// Priority tier for a requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RequirementPriority {
    /// Blocking — must be verified before release.
    P0,
    /// High priority.
    P1,
    /// Medium priority.
    P2,
    /// Low or nice-to-have.
    P3,
}

/// A piece of evidence demonstrating a requirement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerificationEvidence {
    /// Human-readable label for this evidence.
    pub label: String,
    /// Path or reference to the evidence artifact.
    pub artifact: String,
}

/// A criterion not yet satisfied.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerificationGap {
    /// Description of the missing criteria.
    pub missing_criteria: String,
    /// Suggested action to close the gap.
    pub suggested_action: String,
}

/// Verification result for a single requirement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequirementVerification {
    /// Requirement identifier.
    pub id: String,
    /// Human-readable title.
    pub title: String,
    /// Version tier (e.g., `"v1"`, `"v2"`).
    pub tier: String,
    /// Priority level.
    pub priority: RequirementPriority,
    /// Current verification status.
    pub status: VerificationStatus,
    /// Coverage percentage 0–100.
    pub coverage_pct: u8,
    /// Evidence supporting this requirement.
    pub evidence: Vec<VerificationEvidence>,
    /// Gaps remaining for this requirement.
    pub gaps: Vec<VerificationGap>,
}

/// Full verification result for a project.
///
/// Wire format consumed by the desktop `VerificationView` and served
/// by pylon's `GET /api/planning/projects/{id}/verification`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectVerificationResult {
    /// Project identifier.
    pub project_id: String,
    /// Per-requirement verification results.
    pub requirements: Vec<RequirementVerification>,
    /// ISO 8601 timestamp of the last verification run.
    pub last_verified_at: String,
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test assertions may panic on failure"
)]
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
        let json =
            r#"{"id": "syn", "name": "Syn", "model": "claude-opus-4-6", "emoji": "\ud83e\udde0"}"#;
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
    }

    #[test]
    fn nous_tools_response_deserialization() {
        let json = r#"{"tools": [{"name": "read_file", "enabled": true}, {"name": "bash", "enabled": false}]}"#;
        let resp: NousToolsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.tools.len(), 2);
        assert!(resp.tools[0].enabled);
        assert!(!resp.tools[1].enabled);
    }

    // -- verification types --

    #[test]
    fn verification_result_serde_roundtrip() {
        let result = ProjectVerificationResult {
            project_id: "p1".to_string(),
            requirements: vec![RequirementVerification {
                id: "r1".to_string(),
                title: "Tests pass".to_string(),
                tier: "v1".to_string(),
                priority: RequirementPriority::P0,
                status: VerificationStatus::Verified,
                coverage_pct: 100,
                evidence: vec![VerificationEvidence {
                    label: "CI run".to_string(),
                    artifact: "run-123".to_string(),
                }],
                gaps: vec![],
            }],
            last_verified_at: "2026-01-01T00:00:00Z".to_string(),
        };
        let json = serde_json::to_string(&result).unwrap();
        let back: ProjectVerificationResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back, result);
    }

    #[test]
    fn verification_status_snake_case_roundtrip() {
        let statuses = [
            (VerificationStatus::Verified, "\"verified\""),
            (
                VerificationStatus::PartiallyVerified,
                "\"partially_verified\"",
            ),
            (VerificationStatus::Unverified, "\"unverified\""),
            (VerificationStatus::Failed, "\"failed\""),
        ];
        for (status, expected_json) in &statuses {
            let json = serde_json::to_string(status).unwrap();
            assert_eq!(&json, *expected_json);
            let back: VerificationStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(back, *status);
        }
    }

    #[test]
    fn requirement_priority_snake_case_roundtrip() {
        let priorities = [
            (RequirementPriority::P0, "\"p0\""),
            (RequirementPriority::P1, "\"p1\""),
            (RequirementPriority::P2, "\"p2\""),
            (RequirementPriority::P3, "\"p3\""),
        ];
        for (priority, expected_json) in &priorities {
            let json = serde_json::to_string(priority).unwrap();
            assert_eq!(&json, *expected_json);
            let back: RequirementPriority = serde_json::from_str(&json).unwrap();
            assert_eq!(back, *priority);
        }
    }

    #[test]
    fn verification_gap_serde() {
        let gap = VerificationGap {
            missing_criteria: "test coverage".to_string(),
            suggested_action: "add integration tests".to_string(),
        };
        let json = serde_json::to_string(&gap).unwrap();
        let back: VerificationGap = serde_json::from_str(&json).unwrap();
        assert_eq!(back, gap);
    }

    #[test]
    fn empty_verification_result_deserializes() {
        let json = r#"{
            "project_id": "p1",
            "requirements": [],
            "last_verified_at": "pending"
        }"#;
        let result: ProjectVerificationResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.project_id, "p1");
        assert!(result.requirements.is_empty());
        assert_eq!(result.last_verified_at, "pending");
    }
}
