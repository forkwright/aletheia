//! Chiron (Χείρων): self-auditing loop via prosoche checks.
//!
//! Implements periodic, event-based, and manual audit checks that assess
//! agent behavior and store results in the knowledge graph. Failed checks
//! surface as structured observations fed back into the nous pipeline.

pub mod checks;

use std::sync::atomic::{AtomicU32, Ordering};

use serde::{Deserialize, Serialize};

/// Status of an audit check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum CheckStatus {
    /// Check passed: metric is within acceptable bounds.
    Pass,
    /// Check produced a warning: metric is degraded but not critical.
    Warn,
    /// Check failed: metric is below acceptable threshold.
    Fail,
}

impl std::fmt::Display for CheckStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pass => f.write_str("pass"),
            Self::Warn => f.write_str("warn"),
            Self::Fail => f.write_str("fail"),
        }
    }
}

/// Result of running a single prosoche check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    /// Overall status.
    pub status: CheckStatus,
    /// Numeric score between 0.0 (worst) and 1.0 (best).
    pub score: f64,
    /// Human-readable evidence describing the outcome.
    pub evidence: String,
}

/// Record of a tool call outcome used by audit checks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    /// Name of the tool that was called.
    pub tool_name: String,
    /// Whether the call succeeded.
    pub success: bool,
}

/// Context provided to audit checks during execution.
///
/// The caller populates this from session and knowledge stores before
/// invoking the auditor. Checks are pure functions of this context.
#[derive(Debug, Clone, Default)]
pub struct CheckContext {
    /// Which nous is being audited.
    pub nous_id: String,
    /// Recent tool call outcomes for this nous.
    pub recent_tool_calls: Vec<ToolCallRecord>,
    /// Recent assistant response lengths (in characters).
    pub recent_response_lengths: Vec<usize>,
    /// Total fact count in the knowledge graph.
    pub fact_count: usize,
    /// Count of facts with missing or invalid temporal bounds.
    pub temporal_violation_count: usize,
    /// Count of broken supersession chains (`superseded_by` points to nonexistent fact).
    pub broken_chain_count: usize,
}

/// What triggered an audit run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum AuditTrigger {
    /// Time-based: periodic interval elapsed.
    Periodic {
        /// Configured interval in seconds.
        interval_secs: u64,
    },
    /// Event-based: agent performed N actions since last audit.
    EventBased {
        /// Number of actions that triggered this audit.
        after_n_actions: u32,
    },
    /// Manual trigger via CLI or API.
    Manual,
}

impl std::fmt::Display for AuditTrigger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Periodic { interval_secs } => write!(f, "periodic({interval_secs}s)"),
            Self::EventBased { after_n_actions } => {
                write!(f, "event-based(after {after_n_actions} actions)")
            }
            Self::Manual => f.write_str("manual"),
        }
    }
}

/// Result of a single check within an audit report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditCheckResult {
    /// Name of the check that ran.
    pub check_name: String,
    /// Description of what the check verifies.
    pub check_description: String,
    /// The check outcome.
    pub result: CheckResult,
}

/// Complete report from an audit run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditReport {
    /// Which nous was audited.
    pub nous_id: String,
    /// What triggered this audit.
    pub trigger: AuditTrigger,
    /// Individual check results.
    pub results: Vec<AuditCheckResult>,
    /// ISO 8601 timestamp when the audit completed.
    pub checked_at: String,
}

impl AuditReport {
    /// Iterate over checks that did not pass.
    pub fn failed_checks(&self) -> impl Iterator<Item = &AuditCheckResult> {
        self.results
            .iter()
            .filter(|r| r.result.status != CheckStatus::Pass)
    }

    /// Format failed and warning checks as structured observations for the nous pipeline.
    ///
    /// Each observation is a human-readable string suitable for injection into
    /// the agent's context as a system-level note.
    #[must_use]
    pub fn to_observations(&self) -> Vec<String> {
        self.failed_checks()
            .map(|r| {
                format!(
                    "[chiron-audit] {}: {} (score: {:.2}, status: {}). Evidence: {}",
                    r.check_name,
                    r.check_description,
                    r.result.score,
                    r.result.status,
                    r.result.evidence,
                )
            })
            .collect()
    }
}

/// A self-audit check that evaluates a specific aspect of agent behavior.
///
/// Implementations analyze the [`CheckContext`] and return a [`CheckResult`]
/// indicating pass/warn/fail with a numeric score and evidence string.
pub trait ProsocheCheck: Send + Sync {
    /// Unique name for this check (e.g., `"knowledge_consistency"`).
    fn name(&self) -> &'static str;

    /// Human-readable description of what this check verifies.
    fn description(&self) -> &'static str;

    /// Run the check against the provided context.
    fn run(&self, ctx: &CheckContext) -> CheckResult;
}

/// Default number of agent actions between event-based audit triggers.
const DEFAULT_EVENT_THRESHOLD: u32 = 50;

/// Chiron self-auditor: manages registered checks and trigger logic.
pub struct ChironAuditor {
    checks: Vec<Box<dyn ProsocheCheck>>,
    action_counter: AtomicU32,
    event_threshold: u32,
}

impl ChironAuditor {
    /// Create a new auditor with no registered checks.
    #[must_use]
    pub fn new() -> Self {
        Self {
            checks: Vec::new(),
            action_counter: AtomicU32::new(0),
            event_threshold: DEFAULT_EVENT_THRESHOLD,
        }
    }

    /// Set the event-based trigger threshold (number of actions between audits).
    #[must_use]
    pub fn with_event_threshold(mut self, n: u32) -> Self {
        self.event_threshold = n;
        self
    }

    /// Register a check with the auditor.
    pub fn register(&mut self, check: Box<dyn ProsocheCheck>) {
        self.checks.push(check);
    }

    /// Register the three default checks.
    pub fn register_defaults(&mut self) {
        self.register(Box::new(checks::KnowledgeConsistencyCheck));
        self.register(Box::new(checks::ToolSuccessRateCheck));
        self.register(Box::new(checks::ResponseQualityCheck));
    }

    /// Record an agent action and return `true` if the event-based threshold
    /// has been reached (caller should trigger an audit).
    pub fn record_action(&self) -> bool {
        let prev = self.action_counter.fetch_add(1, Ordering::Relaxed);
        let new_count = prev.saturating_add(1);
        if new_count >= self.event_threshold {
            self.action_counter.store(0, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    /// Run all registered checks and produce an audit report.
    #[must_use]
    pub fn run_audit(&self, ctx: &CheckContext, trigger: AuditTrigger) -> AuditReport {
        let results = self
            .checks
            .iter()
            .map(|check| AuditCheckResult {
                check_name: check.name().to_owned(),
                check_description: check.description().to_owned(),
                result: check.run(ctx),
            })
            .collect();

        AuditReport {
            nous_id: ctx.nous_id.clone(),
            trigger,
            results,
            checked_at: jiff::Timestamp::now().to_string(),
        }
    }

    /// Number of registered checks.
    #[must_use]
    pub fn check_count(&self) -> usize {
        self.checks.len()
    }
}

impl Default for ChironAuditor {
    fn default() -> Self {
        Self::new()
    }
}

/// Store an audit report as facts in the knowledge graph.
///
/// Each non-passing check result is persisted as an `Audit`-type fact with
/// bi-temporal timestamps. Passing checks are not stored to avoid noise.
///
/// # Errors
///
/// Returns an error if inserting a fact into the knowledge store fails.
#[cfg(feature = "knowledge-store")]
pub fn store_audit_report(
    knowledge_store: &aletheia_mneme::knowledge_store::KnowledgeStore,
    report: &AuditReport,
) -> crate::error::Result<()> {
    use aletheia_mneme::knowledge::{EpistemicTier, far_future};
    use snafu::ResultExt;

    let now = jiff::Timestamp::now();

    for check_result in &report.results {
        let content = serde_json::json!({
            "check": check_result.check_name,
            "description": check_result.check_description,
            "status": check_result.result.status,
            "score": check_result.result.score,
            "evidence": check_result.result.evidence,
            "trigger": report.trigger,
        })
        .to_string();

        let confidence = check_result.result.score;
        let tier = match check_result.result.status {
            CheckStatus::Pass => EpistemicTier::Verified,
            CheckStatus::Warn => EpistemicTier::Inferred,
            CheckStatus::Fail => EpistemicTier::Assumed,
        };

        let fact_id = aletheia_mneme::id::FactId::from(format!("audit-{}", ulid::Ulid::new()));

        let fact = aletheia_mneme::knowledge::Fact {
            id: fact_id,
            nous_id: report.nous_id.clone(),
            fact_type: String::from("audit"),
            content,
            confidence,
            tier,
            valid_from: now,
            valid_to: far_future(),
            recorded_at: now,
            source_session_id: None,
            superseded_by: None,
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
            access_count: 0,
            last_accessed_at: None,
            stability_hours: aletheia_mneme::knowledge::FactType::Audit.base_stability_hours(),
        };

        knowledge_store
            .insert_fact(&fact)
            .context(crate::error::StoreSnafu)?;
    }

    tracing::info!(
        nous_id = %report.nous_id,
        checks = report.results.len(),
        failed = report.failed_checks().count(),
        "chiron audit results stored"
    );

    Ok(())
}

/// Query audit history from the knowledge graph.
///
/// Returns facts with `fact_type = "audit"` for the given nous, ordered by
/// most recent first.
///
/// # Errors
///
/// Returns an error if the knowledge store query fails.
#[cfg(feature = "knowledge-store")]
pub fn query_audit_history(
    knowledge_store: &aletheia_mneme::knowledge_store::KnowledgeStore,
    nous_id: &str,
    limit: usize,
) -> crate::error::Result<Vec<aletheia_mneme::knowledge::Fact>> {
    use snafu::ResultExt;

    let limit_i64 = i64::try_from(limit).unwrap_or(i64::MAX);
    knowledge_store
        .query_facts_by_type(nous_id, "audit", limit_i64)
        .context(crate::error::StoreSnafu)
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    #[test]
    fn check_status_display() {
        assert_eq!(CheckStatus::Pass.to_string(), "pass");
        assert_eq!(CheckStatus::Warn.to_string(), "warn");
        assert_eq!(CheckStatus::Fail.to_string(), "fail");
    }

    #[test]
    fn audit_trigger_display() {
        let periodic = AuditTrigger::Periodic { interval_secs: 300 };
        assert!(
            periodic.to_string().contains("300"),
            "periodic trigger should display interval"
        );

        let event = AuditTrigger::EventBased {
            after_n_actions: 50,
        };
        assert!(
            event.to_string().contains("50"),
            "event trigger should display action count"
        );

        let manual = AuditTrigger::Manual;
        assert_eq!(manual.to_string(), "manual");
    }

    #[test]
    fn chiron_auditor_register_defaults() {
        let mut auditor = ChironAuditor::new();
        auditor.register_defaults();
        assert_eq!(
            auditor.check_count(),
            3,
            "should register exactly three default checks"
        );
    }

    #[test]
    fn chiron_auditor_record_action_triggers_at_threshold() {
        let auditor = ChironAuditor::new().with_event_threshold(3);
        assert!(!auditor.record_action(), "1st action should not trigger");
        assert!(!auditor.record_action(), "2nd action should not trigger");
        assert!(auditor.record_action(), "3rd action should trigger audit");
        assert!(
            !auditor.record_action(),
            "counter should reset after trigger"
        );
    }

    #[test]
    fn run_audit_produces_report() {
        let mut auditor = ChironAuditor::new();
        auditor.register_defaults();
        let ctx = CheckContext {
            nous_id: String::from("test-nous"),
            ..Default::default()
        };
        let report = auditor.run_audit(&ctx, AuditTrigger::Manual);
        assert_eq!(report.nous_id, "test-nous");
        assert_eq!(
            report.results.len(),
            3,
            "report should have one result per check"
        );
        assert!(!report.checked_at.is_empty(), "checked_at should be set");
    }

    #[test]
    fn audit_report_to_observations_includes_failures() {
        let report = AuditReport {
            nous_id: String::from("test-nous"),
            trigger: AuditTrigger::Manual,
            results: vec![
                AuditCheckResult {
                    check_name: String::from("passing_check"),
                    check_description: String::from("always passes"),
                    result: CheckResult {
                        status: CheckStatus::Pass,
                        score: 1.0,
                        evidence: String::from("all good"),
                    },
                },
                AuditCheckResult {
                    check_name: String::from("failing_check"),
                    check_description: String::from("always fails"),
                    result: CheckResult {
                        status: CheckStatus::Fail,
                        score: 0.2,
                        evidence: String::from("something wrong"),
                    },
                },
            ],
            checked_at: String::from("2026-03-19T00:00:00Z"),
        };

        let observations = report.to_observations();
        assert_eq!(
            observations.len(),
            1,
            "only non-passing checks should produce observations"
        );
        assert!(
            observations
                .first()
                .is_some_and(|o| o.contains("failing_check")),
            "observation should name the failed check"
        );
    }

    #[test]
    fn audit_report_serialization_roundtrip() {
        let report = AuditReport {
            nous_id: String::from("test-nous"),
            trigger: AuditTrigger::Manual,
            results: vec![AuditCheckResult {
                check_name: String::from("test"),
                check_description: String::from("test check"),
                result: CheckResult {
                    status: CheckStatus::Pass,
                    score: 1.0,
                    evidence: String::from("ok"),
                },
            }],
            checked_at: String::from("2026-03-19T00:00:00Z"),
        };
        let json = serde_json::to_string(&report).expect("serialize should succeed");
        let back: AuditReport = serde_json::from_str(&json).expect("deserialize should succeed");
        assert_eq!(back.nous_id, "test-nous");
        assert_eq!(back.results.len(), 1);
    }

    #[test]
    fn default_auditor_has_no_checks() {
        let auditor = ChironAuditor::default();
        assert_eq!(
            auditor.check_count(),
            0,
            "default auditor should start empty"
        );
    }
}
