//! Record types for energeia state persistence.
//!
//! These structs are the value-side of the key-value store. Each record is
//! serialized via `MessagePack` for compact binary storage in fjall.

use serde::{Deserialize, Serialize};

use koina::newtype_id;

use crate::types::{FailureClass, QaVerdict, SessionStatus};

newtype_id!(
    /// Unique identifier for a dispatch run (ULID, time-sortable).
    pub struct DispatchId(String)
);

newtype_id!(
    /// Unique identifier for a session within a dispatch (ULID, time-sortable).
    pub struct PromptSessionId(String)
);

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

/// Persistent state of a single session within a dispatch.
///
/// WHY(#4800): preserves optional terminal prompt attribution that would
/// otherwise exist only in the in-memory dispatch result. This record is not
/// yet a crash-safe checkpoint or a complete per-attempt execution history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    /// Unique identifier for this session.
    pub id: PromptSessionId,
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
    // INVARIANT(#4800): `rmp_serde::to_vec` (used by `serialize_msgpack`, not
    // `to_vec_named`) encodes structs positionally as arrays, not as maps
    // keyed by field name. Every field below this point was added after
    // `updated_at`, which is load-bearing: a shorter array from a
    // pre-existing on-disk record decodes fine because serde fills exhausted
    // *trailing* positions from `#[serde(default)]`, but it cannot do that
    // for a field inserted in the middle -- every following field would
    // silently decode from the wrong array slot. New fields on this struct
    // must always be appended here, never inserted above.
    /// LLM model used for this session, if known.
    #[serde(default)]
    pub model: Option<String>,
    /// Typed reason bucket for failed sessions.
    #[serde(default)]
    pub failure_class: Option<FailureClass>,
    /// Number of times the session was resumed via health checks.
    #[serde(default)]
    pub resume_count: u32,
    /// Number of QA-driven corrective attempts made for this prompt.
    #[serde(default)]
    pub corrective_attempts: u32,
    /// Tokens read from the prompt cache on this session.
    #[serde(default)]
    pub cache_hit_tokens: u64,
    /// Tokens written to the prompt cache on this session.
    #[serde(default)]
    pub cache_miss_tokens: u64,
    /// Parsed structured output from this session, if the prompt declared
    /// an output format and the final result was valid JSON.
    #[serde(default)]
    pub structured_output: Option<serde_json::Value>,
}

/// Fields that can be updated on a session after creation.
///
/// `None` means "leave unchanged." This update shape cannot explicitly clear
/// an optional field once it has been persisted.
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
    /// LLM model used for this session.
    pub model: Option<String>,
    /// Typed reason bucket for a failed session.
    pub failure_class: Option<FailureClass>,
    /// Updated resume count.
    pub resume_count: Option<u32>,
    /// Updated corrective-attempt count.
    pub corrective_attempts: Option<u32>,
    /// Updated prompt-cache hit token count.
    pub cache_hit_tokens: Option<u64>,
    /// Updated prompt-cache miss token count.
    pub cache_miss_tokens: Option<u64>,
    /// Parsed structured output from the session.
    pub structured_output: Option<serde_json::Value>,
}

/// A dispatch record bundled with all of its child session records.
///
/// WHY(#4800): store consumers need the parent dispatch and its currently
/// persisted prompt-level session records without knowing the fjall key
/// layout. User-facing inspection and complete attempt history remain
/// separate work.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchExport {
    /// The parent dispatch record.
    pub dispatch: DispatchRecord,
    /// All session records belonging to this dispatch, ordered by prompt number.
    pub sessions: Vec<SessionRecord>,
}

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

/// An observation captured during dispatch execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservationRecord {
    /// Unique identifier for this observation.
    // kanon:ignore RUST/primitive-for-domain-id — public record type persisted to store; changing to newtype would require migration and breaking API change
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

/// Result of a CI validation check against a session's PR.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CiValidationRecord {
    /// Session this validation relates to.
    pub session_id: PromptSessionId,
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

/// Persisted QA verdict emitted during dispatch post-processing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QaVerdictRecord {
    /// Parent dispatch this verdict belongs to.
    pub dispatch_id: DispatchId,
    /// Project slug this verdict belongs to.
    pub project: String,
    /// Overall QA verdict.
    pub verdict: QaVerdict,
    /// Timestamp when the verdict was recorded.
    pub recorded_at: jiff::Timestamp,
}

impl std::fmt::Display for CiValidationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pass => write!(f, "pass"),
            Self::Fail => write!(f, "fail"),
        }
    }
}

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
    /// Number of QA-driven corrective attempts made for this prompt.
    #[serde(default)]
    pub corrective_attempts: u32,
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::float_cmp, reason = "test assertions on exact float values")]
mod tests {
    use super::*;

    #[test]
    fn dispatch_id_roundtrip() {
        let id = DispatchId::new("01JQXYZ123").unwrap();
        let json = serde_json::to_string(&id).unwrap();
        let back: DispatchId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn dispatch_id_new_rejects_empty() {
        assert!(DispatchId::new("").is_err());
    }

    #[test]
    fn session_id_new_rejects_empty() {
        assert!(PromptSessionId::new("").is_err());
    }

    #[test]
    fn session_id_roundtrip() {
        let id = PromptSessionId::new("01JQXYZ456").unwrap();
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
            id: DispatchId::new("01JQXYZ123").unwrap(),
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
            id: PromptSessionId::new("01JQSESS01").unwrap(),
            dispatch_id: DispatchId::new("01JQXYZ123").unwrap(),
            prompt_number: 1,
            status: SessionStatus::Success,
            session_id: Some("cc-sess-abc".to_owned()),
            cost_usd: 0.42,
            num_turns: 15,
            duration_ms: 30_000,
            pr_url: Some("https://github.com/acme/repo/pull/42".to_owned()),
            error: None,
            model: Some("claude-3-5-sonnet".to_owned()),
            failure_class: None,
            resume_count: 2,
            corrective_attempts: 1,
            cache_hit_tokens: 500,
            cache_miss_tokens: 100,
            structured_output: Some(serde_json::json!({"kind": "feature"})),
            created_at: jiff::Timestamp::now(),
            updated_at: jiff::Timestamp::now(),
        };
        let bytes = rmp_serde::to_vec(&record).unwrap();
        let back: SessionRecord = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(back.prompt_number, 1);
        assert_eq!(back.cost_usd, 0.42);
        assert_eq!(back.model.as_deref(), Some("claude-3-5-sonnet"));
        assert_eq!(back.resume_count, 2);
        assert_eq!(back.corrective_attempts, 1);
        assert_eq!(back.cache_hit_tokens, 500);
        assert_eq!(back.cache_miss_tokens, 100);
        assert_eq!(
            back.structured_output,
            Some(serde_json::json!({"kind": "feature"}))
        );
    }

    #[test]
    fn session_record_deserializes_from_pre_attribution_msgpack() {
        // WHY(#4800): records written before this field set existed must still
        // deserialize -- the new fields all carry `#[serde(default)]`. Simulate
        // an old record by hand-encoding only the original field set.
        #[derive(serde::Serialize)]
        struct LegacySessionRecord {
            id: PromptSessionId,
            dispatch_id: DispatchId,
            prompt_number: u32,
            status: SessionStatus,
            session_id: Option<String>,
            cost_usd: f64,
            num_turns: u32,
            duration_ms: u64,
            pr_url: Option<String>,
            error: Option<String>,
            created_at: jiff::Timestamp,
            updated_at: jiff::Timestamp,
        }

        let legacy = LegacySessionRecord {
            id: PromptSessionId::new("01JQSESS02").unwrap(),
            dispatch_id: DispatchId::new("01JQXYZ456").unwrap(),
            prompt_number: 2,
            status: SessionStatus::Success,
            session_id: None,
            cost_usd: 0.10,
            num_turns: 3,
            duration_ms: 1_000,
            pr_url: None,
            error: None,
            created_at: jiff::Timestamp::now(),
            updated_at: jiff::Timestamp::now(),
        };
        let bytes = rmp_serde::to_vec(&legacy).unwrap();
        let back: SessionRecord = rmp_serde::from_slice(&bytes).unwrap();

        assert_eq!(back.prompt_number, 2);
        assert!(back.model.is_none());
        assert!(back.failure_class.is_none());
        assert_eq!(back.resume_count, 0);
        assert_eq!(back.corrective_attempts, 0);
        assert_eq!(back.cache_hit_tokens, 0);
        assert_eq!(back.cache_miss_tokens, 0);
        assert!(back.structured_output.is_none());
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
