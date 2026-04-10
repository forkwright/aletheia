//! Record types for energeia state persistence.
//!
//! These structs are the value-side of the key-value store. Each record is
//! serialized via `MessagePack` for compact binary storage in fjall.

use serde::{Deserialize, Serialize};

use koina::newtype_id;

use crate::types::SessionStatus;

// ---------------------------------------------------------------------------
// Domain IDs
// ---------------------------------------------------------------------------

newtype_id!(
    /// Unique identifier for a dispatch run (ULID, time-sortable).
    pub struct DispatchId(String)
);

newtype_id!(
    /// Unique identifier for a session within a dispatch (ULID, time-sortable).
    pub struct SessionId(String)
);

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// Persistent state of a dispatch lifecycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchRecord {
    /// Unique identifier for this dispatch.
    pub id: DispatchId,
    /// Project slug (owner/repo) this dispatch belongs to.
    pub project: String,
    /// Serialized dispatch specification (JSON).
    pub spec: String,
    /// Current lifecycle status of the dispatch.
    pub status: DispatchStatus,
    /// Timestamp when the dispatch was created.
    pub created_at: jiff::Timestamp,
    /// Timestamp when the dispatch finished, if completed.
    pub finished_at: Option<jiff::Timestamp>,
    /// Total cost in USD across all sessions in this dispatch.
    pub total_cost_usd: f64,
    /// Total number of sessions in this dispatch.
    pub total_sessions: u32,
}

/// Lifecycle status of a dispatch run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum DispatchStatus {
    /// Dispatch is currently in progress.
    Running,
    /// Dispatch completed successfully.
    Completed,
    /// Dispatch failed or was aborted.
    Failed,
}

impl std::fmt::Display for DispatchStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Running => write!(f, "running"),
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

// ---------------------------------------------------------------------------
// Session
// ---------------------------------------------------------------------------

/// Persistent state of a single session within a dispatch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    /// Unique identifier for this session.
    pub id: SessionId,
    /// Parent dispatch this session belongs to.
    pub dispatch_id: DispatchId,
    /// Prompt number this session is executing.
    pub prompt_number: u32,
    /// Current execution status of the session.
    pub status: SessionStatus,
    /// Claude Code session identifier, set after agent starts.
    pub session_id: Option<String>,
    /// Cost in USD for this session.
    pub cost_usd: f64,
    /// Number of turns (agent iterations) in this session.
    pub num_turns: u32,
    /// Duration of the session in milliseconds.
    pub duration_ms: u64,
    /// URL of the PR created by this session, if any.
    pub pr_url: Option<String>,
    /// Error message if the session failed.
    pub error: Option<String>,
    /// Timestamp when the session was created.
    pub created_at: jiff::Timestamp,
    /// Timestamp of the last update to this session.
    pub updated_at: jiff::Timestamp,
}

/// Fields that can be updated on a session after creation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionUpdate {
    /// New status for the session, if changed.
    pub status: Option<SessionStatus>,
    /// Claude Code session identifier, once known.
    pub session_id: Option<String>,
    /// Updated cost in USD.
    pub cost_usd: Option<f64>,
    /// Updated turn count.
    pub num_turns: Option<u32>,
    /// Updated duration in milliseconds.
    pub duration_ms: Option<u64>,
    /// PR URL created by the session.
    pub pr_url: Option<String>,
    /// Error message if the session failed.
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Lesson
// ---------------------------------------------------------------------------

/// A lesson learned from dispatch execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LessonRecord {
    /// Source of the lesson (e.g., "steward", "qa").
    pub source: String,
    /// Category for grouping related lessons.
    pub category: String,
    /// The lesson text itself.
    pub lesson: String,
    /// Supporting evidence or context.
    pub evidence: Option<String>,
    /// Project this lesson relates to, if any.
    pub project: Option<String>,
    /// Prompt number this lesson relates to, if any.
    pub prompt_number: Option<u32>,
    /// Timestamp when the lesson was recorded.
    pub created_at: jiff::Timestamp,
}

/// Input for creating a new lesson.
#[derive(Debug, Clone)]
pub struct NewLesson {
    /// Source of the lesson (e.g., "steward", "qa").
    pub source: String,
    /// Category for grouping related lessons.
    pub category: String,
    /// The lesson text itself.
    pub lesson: String,
    /// Supporting evidence or context.
    pub evidence: Option<String>,
    /// Project this lesson relates to, if any.
    pub project: Option<String>,
    /// Prompt number this lesson relates to, if any.
    pub prompt_number: Option<u32>,
}

// ---------------------------------------------------------------------------
// Observation
// ---------------------------------------------------------------------------

/// An observation captured during dispatch execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservationRecord {
    /// Unique identifier for this observation.
    pub id: String,
    /// Project this observation relates to.
    pub project: String,
    /// Source that captured the observation.
    pub source: String,
    /// Content of the observation.
    pub content: String,
    /// Type of observation (e.g., "bug", "insight").
    pub observation_type: String,
    /// Session ID that produced this observation, if any.
    pub session_id: Option<String>,
    /// Timestamp when the observation was recorded.
    pub created_at: jiff::Timestamp,
}

/// Input for creating a new observation.
#[derive(Debug, Clone)]
pub struct NewObservation {
    /// Project this observation relates to.
    pub project: String,
    /// Source that captured the observation.
    pub source: String,
    /// Content of the observation.
    pub content: String,
    /// Type of observation (e.g., "bug", "insight").
    pub observation_type: String,
    /// Session ID that produced this observation, if any.
    pub session_id: Option<String>,
}

// ---------------------------------------------------------------------------
// CI Validation
// ---------------------------------------------------------------------------

/// Result of a CI validation check against a session's PR.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CiValidationRecord {
    /// Session this validation relates to.
    pub session_id: SessionId,
    /// Name of the CI check (e.g., "build", "test").
    pub check_name: String,
    /// PR number that was validated.
    pub pr_number: u64,
    /// Outcome of the validation.
    pub status: CiValidationStatus,
    /// Additional details about the validation result.
    pub details: Option<String>,
    /// Timestamp when the validation was recorded.
    pub validated_at: jiff::Timestamp,
}

/// Outcome of a CI validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum CiValidationStatus {
    /// CI validation passed.
    Pass,
    /// CI validation failed.
    Fail,
}

impl std::fmt::Display for CiValidationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pass => write!(f, "pass"),
            Self::Fail => write!(f, "fail"),
        }
    }
}

// ---------------------------------------------------------------------------
// Training data
// ---------------------------------------------------------------------------

/// Outcome summary for training data extraction from a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionOutcomeData {
    /// Prompt number this session executed.
    pub prompt_number: u32,
    /// Final status of the session.
    pub status: SessionStatus,
    /// Cost in USD for this session.
    pub cost_usd: f64,
    /// Number of turns (agent iterations) in this session.
    pub num_turns: u32,
    /// Duration of the session in milliseconds.
    pub duration_ms: u64,
    /// URL of the PR created by this session, if any.
    pub pr_url: Option<String>,
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::float_cmp, reason = "test assertions on exact float values")]
mod tests {
    use super::*;

    #[test]
    fn dispatch_id_roundtrip() {
        let id = DispatchId::new("01JQXYZ123");
        let json = serde_json::to_string(&id).unwrap();
        let back: DispatchId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn session_id_roundtrip() {
        let id = SessionId::new("01JQXYZ456");
        assert_eq!(id.as_str(), "01JQXYZ456");
    }

    #[test]
    fn dispatch_status_display() {
        assert_eq!(DispatchStatus::Running.to_string(), "running");
        assert_eq!(DispatchStatus::Completed.to_string(), "completed");
        assert_eq!(DispatchStatus::Failed.to_string(), "failed");
    }

    #[test]
    fn ci_validation_status_display() {
        assert_eq!(CiValidationStatus::Pass.to_string(), "pass");
        assert_eq!(CiValidationStatus::Fail.to_string(), "fail");
    }

    #[test]
    fn dispatch_record_msgpack_roundtrip() {
        let record = DispatchRecord {
            id: DispatchId::new("01JQXYZ123"),
            project: "acme".to_owned(),
            spec: r#"{"prompts":[1,2]}"#.to_owned(),
            status: DispatchStatus::Running,
            created_at: jiff::Timestamp::now(),
            finished_at: None,
            total_cost_usd: 0.0,
            total_sessions: 0,
        };
        let bytes = rmp_serde::to_vec(&record).unwrap();
        let back: DispatchRecord = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(back.id, record.id);
        assert_eq!(back.project, "acme");
    }

    #[test]
    fn session_record_msgpack_roundtrip() {
        let record = SessionRecord {
            id: SessionId::new("01JQSESS01"),
            dispatch_id: DispatchId::new("01JQXYZ123"),
            prompt_number: 1,
            status: SessionStatus::Success,
            session_id: Some("cc-sess-abc".to_owned()),
            cost_usd: 0.42,
            num_turns: 15,
            duration_ms: 30_000,
            pr_url: Some("https://github.com/acme/repo/pull/42".to_owned()),
            error: None,
            created_at: jiff::Timestamp::now(),
            updated_at: jiff::Timestamp::now(),
        };
        let bytes = rmp_serde::to_vec(&record).unwrap();
        let back: SessionRecord = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(back.prompt_number, 1);
        assert_eq!(back.cost_usd, 0.42);
    }

    #[test]
    fn lesson_record_msgpack_roundtrip() {
        let record = LessonRecord {
            source: "steward".to_owned(),
            category: "testing".to_owned(),
            lesson: "Always run clippy before pushing".to_owned(),
            evidence: Some("PR #42 failed CI".to_owned()),
            project: Some("acme".to_owned()),
            prompt_number: Some(3),
            created_at: jiff::Timestamp::now(),
        };
        let bytes = rmp_serde::to_vec(&record).unwrap();
        let back: LessonRecord = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(back.source, "steward");
        assert_eq!(back.lesson, "Always run clippy before pushing");
    }

    #[test]
    fn observation_record_msgpack_roundtrip() {
        let record = ObservationRecord {
            id: "01JQOBS001".to_owned(),
            project: "acme".to_owned(),
            source: "qa".to_owned(),
            content: "Flaky test in auth module".to_owned(),
            observation_type: "bug".to_owned(),
            session_id: None,
            created_at: jiff::Timestamp::now(),
        };
        let bytes = rmp_serde::to_vec(&record).unwrap();
        let back: ObservationRecord = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(back.observation_type, "bug");
    }

    #[test]
    fn session_update_default_is_all_none() {
        let update = SessionUpdate::default();
        assert!(update.status.is_none());
        assert!(update.session_id.is_none());
        assert!(update.cost_usd.is_none());
    }
}
