// kanon:ignore RUST/file-too-long — cohesive prosoche audit framework: trait + impls + report structs belong together
//! Prosoche self-audit framework: structured attention-quality checks.
//!
//! Five check types (consistency, staleness, goal alignment, session quality,
//! instinct patterns) implement [`ProsocheCheck`]; a [`ProsocheAuditRunner`]
//! runs all registered checks on demand and persists [`Finding`]s with
//! [`ArtefactMeta`] stamps for operator review.
//!
//! # Philosophy
//!
//! "Attention is a moral act — the quality of attention determines all downstream
//! outcomes." The heartbeat answers "is the system alive?". This module answers
//! "is the system paying attention to the right things?"
//!
//! # Design
//!
//! Each check receives a [`ProsocheState`] snapshot and returns zero or more
//! [`Finding`]s. The runner collects all findings, stamps each with
//! [`ArtefactMeta`] provenance, and persists an [`AuditReport`] to disk.
//!
//! The [`AuditReport`] itself implements [`Stamped`] so the report envelope
//! carries provenance independent of its contained findings.
//!
//! # Object safety
//!
//! `ProsocheCheck::check` returns a `Pin<Box<dyn Future>>` (same pattern as
//! [`DaemonBridge`](crate::bridge::DaemonBridge)) so the trait is object-safe
//! and `Arc<dyn ProsocheCheck>` works without `async_trait`.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use eidos::knowledge::finding::{EvidenceLevel, Finding, FindingStats};
use eidos::meta::{ArtefactMeta, Stamped};
use serde::{Deserialize, Serialize};

/// The five categories of prosoche self-audit check, one per
/// attention-quality dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ProsocheCheckKind {
    /// Detect contradictions between facts in the knowledge store (X and not-X).
    Consistency,
    /// Identify facts or sessions that haven't been touched in N days without rationale.
    Staleness,
    /// Verify recent session turns are advancing stated goals.
    GoalAlignment,
    /// Evaluate whether sessions produce actionable outcomes (error rate, completion rate).
    SessionQuality,
    /// Detect recurring patterns in agent behavior (loops, avoidance, over-confidence).
    ///
    /// v1: stub — defines the trait shape. Full semantics need gnomon weights.
    /// Tracked in follow-up issue.
    InstinctPatterns,
}

impl std::fmt::Display for ProsocheCheckKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Consistency => write!(f, "consistency"),
            Self::Staleness => write!(f, "staleness"),
            Self::GoalAlignment => write!(f, "goal-alignment"),
            Self::SessionQuality => write!(f, "session-quality"),
            Self::InstinctPatterns => write!(f, "instinct-patterns"),
        }
    }
}

/// Snapshot of system state available to prosoche checks.
///
/// Contains the minimal context needed to run attention-quality checks
/// without requiring access to the full daemon or bridge infrastructure.
///
/// # Extending state
///
/// Add new fields as `Option<T>` so existing check implementations compile
/// without change when new state sources become available.
#[derive(Debug, Clone, Default)]
pub struct ProsocheState {
    /// The nous identity this audit is running for.
    // kanon:ignore RUST/primitive-for-domain-id — ProsocheState is an ephemeral audit input struct; nous_id is passed as &str from the runner and converted to String for serialization
    pub nous_id: String,
    /// Known stated goals for this nous (free-text lines).
    ///
    /// Used by [`GoalAlignmentCheck`] for keyword overlap.
    pub stated_goals: Vec<String>,
    /// Recent session summaries (id, turn count, error count, completed flag).
    ///
    /// Used by [`SessionQualityCheck`] and [`GoalAlignmentCheck`].
    pub sessions: Vec<SessionSnapshot>,
    /// Recent facts for consistency and staleness checks.
    ///
    /// Each entry is `(fact_id, content, last_touched_days_ago)`.
    pub facts: Vec<FactSnapshot>,
    /// Current UTC timestamp (ISO 8601), set at audit start.
    pub checked_at: String,
}

/// A minimal fact snapshot for audit checks.
#[derive(Debug, Clone)]
pub struct FactSnapshot {
    /// Stable fact identifier.
    // kanon:ignore RUST/primitive-for-domain-id — FactSnapshot is an ephemeral audit input; fact_id comes from external knowledge graph facts as raw strings
    pub fact_id: String,
    /// Full text content of the fact.
    pub content: String,
    /// How many days ago this fact was last accessed or updated.
    ///
    /// A value of `None` means last-access is unknown.
    pub days_since_touched: Option<f64>,
}

/// A minimal session snapshot for audit checks.
#[derive(Debug, Clone)]
pub struct SessionSnapshot {
    /// Session identifier.
    // kanon:ignore RUST/primitive-for-domain-id — SessionSnapshot is an ephemeral audit input; session_id comes from external session metadata as raw strings
    pub session_id: String,
    /// Total turn count in the session.
    pub turn_count: u32,
    /// Number of turns that resulted in an error response.
    pub error_count: u32,
    /// Whether the session reached a natural completion (vs. abandoned).
    pub completed: bool,
    /// Combined text of all user turns in this session.
    ///
    /// Used for goal-alignment keyword matching.
    pub turn_text: String,
}

/// A single prosoche self-audit check.
///
/// Implementations receive a [`ProsocheState`] snapshot and produce zero or
/// more [`Finding`]s describing attention-quality issues. Each finding carries
/// its own [`EvidenceLevel`] so consumers can weight the results appropriately.
///
/// # Implementation contract
///
/// - Checks MUST be stateless: all input is in [`ProsocheState`].
/// - Checks MUST NOT panic. Return an empty `Vec` on errors and log via `tracing`.
/// - Checks SHOULD log one `tracing::info!` per invocation with `findings_count`.
/// - Checks SHOULD be fast (<100ms). Long-running analysis belongs in a separate
///   maintenance task, not a prosoche check.
///
/// # Object safety
///
/// `check` returns `Pin<Box<dyn Future>>` so the trait is object-safe and
/// `Arc<dyn ProsocheCheck>` works without `async_trait`. Pattern mirrors
/// [`crate::bridge::DaemonBridge`].
pub trait ProsocheCheck: Send + Sync {
    /// Run the check against the provided system state.
    ///
    /// Returns zero or more findings. An empty `Vec` means no issues detected —
    /// it is NOT an error condition.
    fn check<'a>(
        &'a self,
        state: &'a ProsocheState,
    ) -> Pin<Box<dyn std::future::Future<Output = Vec<Finding>> + Send + 'a>>;

    /// The kind of attention this check evaluates.
    fn kind(&self) -> ProsocheCheckKind;
}

/// Detect contradictions in the fact graph.
///
/// v1 heuristic: looks for fact pairs where one fact content contains a term
/// and another contains "not <term>" or "never <term>". This catches obvious
/// logical contradictions without requiring symbolic reasoning.
///
/// Future: integrate with episteme's A-MemGuard consensus layer for full
/// multi-path contradiction detection.
pub struct ConsistencyCheck;

impl ProsocheCheck for ConsistencyCheck {
    #[tracing::instrument(skip(self, state))]
    fn check<'a>(
        &'a self,
        state: &'a ProsocheState,
    ) -> Pin<Box<dyn std::future::Future<Output = Vec<Finding>> + Send + 'a>> {
        Box::pin(async move {
            let mut findings = Vec::new();

            let normalised: Vec<(String, String, Vec<String>)> = state
                .facts
                .iter()
                .map(|f| {
                    let terms = extract_key_terms(&f.content);
                    (f.fact_id.clone(), f.content.clone(), terms)
                })
                .collect();

            for (i, (id_a, content_a, terms_a)) in normalised.iter().enumerate() {
                for (id_b, content_b, _) in normalised.get(i + 1..).unwrap_or_default() {
                    for term in terms_a {
                        let negated = format!("not {term}");
                        let negated_alt = format!("never {term}");
                        let content_b_lower = content_b.to_lowercase();
                        if content_b_lower.contains(&negated)
                            || content_b_lower.contains(&negated_alt)
                        {
                            findings.push(Finding {
                                finding_id: format!("PROSOCHE-CONSISTENCY-{}", findings.len() + 1),
                                claim: format!(
                                    "Fact '{id_a}' asserts '{term}'; \
                                     fact '{id_b}' appears to negate it."
                                ),
                                evidence_level: EvidenceLevel::Exploratory,
                                counter_argument:
                                    "Term-negation heuristic; may be false positive on \
                                     nuanced phrasing. Requires human review."
                                        .to_owned(),
                                source: "prosoche::ConsistencyCheck".to_owned(),
                                stats: FindingStats::none(),
                            });
                            break;
                        }

                        let content_a_lower = content_a.to_lowercase();
                        let neg_in_a = content_a_lower.contains(&negated)
                            || content_a_lower.contains(&negated_alt);
                        if neg_in_a
                            && content_b_lower.split_whitespace().any(|w| {
                                w.trim_matches(|c: char| !c.is_alphanumeric()) == term.as_str()
                            })
                        {
                            findings.push(Finding {
                                finding_id: format!("PROSOCHE-CONSISTENCY-{}", findings.len() + 1),
                                claim: format!(
                                    "Fact '{id_a}' negates '{term}'; \
                                     fact '{id_b}' asserts it."
                                ),
                                evidence_level: EvidenceLevel::Exploratory,
                                counter_argument:
                                    "Term-negation heuristic; may be false positive on \
                                     nuanced phrasing. Requires human review."
                                        .to_owned(),
                                source: "prosoche::ConsistencyCheck".to_owned(),
                                stats: FindingStats::none(),
                            });
                            break;
                        }
                    }
                }
            }

            tracing::info!(
                check_kind = %ProsocheCheckKind::Consistency,
                findings_count = findings.len(),
                "prosoche audit complete"
            );

            findings
        })
    }

    fn kind(&self) -> ProsocheCheckKind {
        ProsocheCheckKind::Consistency
    }
}

/// Extract significant key terms from fact content for contradiction detection.
///
/// Returns lowercase single-word terms longer than 3 characters, excluding
/// common stop-words. Kept minimal for v1: full NLP not required.
fn extract_key_terms(content: &str) -> Vec<String> {
    const STOP_WORDS: &[&str] = &[
        "the", "and", "for", "are", "that", "this", "with", "have", "from", "not", "but", "they",
        "been", "will", "when", "what", "about", "which", "their", "there", "were", "also", "into",
        "than", "then", "some",
    ];

    content
        .split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|w| w.len() > 3 && !STOP_WORDS.contains(&w.as_str()))
        .collect()
}

/// Flag facts and sessions that haven't been touched in N days without a
/// documented rationale.
///
/// v1 thresholds:
/// - Facts untouched > 90 days: `Exploratory` finding.
/// - Incomplete sessions with > 10 turns: `Interpretive` finding.
pub struct StalenessCheck {
    /// Number of days after which an unaccessed fact is considered stale.
    pub fact_stale_days: f64,
}

impl Default for StalenessCheck {
    fn default() -> Self {
        Self {
            fact_stale_days: 90.0,
        }
    }
}

impl ProsocheCheck for StalenessCheck {
    #[tracing::instrument(skip(self, state))]
    fn check<'a>(
        &'a self,
        state: &'a ProsocheState,
    ) -> Pin<Box<dyn std::future::Future<Output = Vec<Finding>> + Send + 'a>> {
        Box::pin(async move {
            let mut findings = Vec::new();

            for fact in &state.facts {
                if let Some(days) = fact.days_since_touched
                    && days > self.fact_stale_days
                {
                    findings.push(Finding {
                        finding_id: format!("PROSOCHE-STALENESS-FACT-{}", findings.len() + 1),
                        claim: format!(
                            "Fact '{}' has not been accessed in {days:.0} days \
                             (threshold: {:.0}).",
                            fact.fact_id, self.fact_stale_days
                        ),
                        evidence_level: EvidenceLevel::Exploratory,
                        counter_argument:
                            "Stale facts may still be valid long-term reference data. \
                             Requires operator review to confirm whether expiry or \
                             archival is appropriate."
                                .to_owned(),
                        source: "prosoche::StalenessCheck".to_owned(),
                        stats: FindingStats::none(),
                    });
                }
            }

            // Future: carry session_age_days in SessionSnapshot and use it here.
            for session in &state.sessions {
                if !session.completed && session.turn_count > 10 {
                    findings.push(Finding {
                        finding_id: format!("PROSOCHE-STALENESS-SESSION-{}", findings.len() + 1),
                        claim: format!(
                            "Session '{}' has {} turns but was never completed.",
                            session.session_id, session.turn_count
                        ),
                        evidence_level: EvidenceLevel::Interpretive,
                        counter_argument:
                            "Long incomplete sessions may reflect legitimate open-ended work. \
                             Requires operator review."
                                .to_owned(),
                        source: "prosoche::StalenessCheck".to_owned(),
                        stats: FindingStats::none(),
                    });
                }
            }

            tracing::info!(
                check_kind = %ProsocheCheckKind::Staleness,
                findings_count = findings.len(),
                "prosoche audit complete"
            );

            findings
        })
    }

    fn kind(&self) -> ProsocheCheckKind {
        ProsocheCheckKind::Staleness
    }
}

/// Verify recent session turns are advancing stated goals.
///
/// v1: keyword-overlap heuristic. For each session, count how many of the
/// stated-goal keywords appear in the session's turn text. Sessions with
/// zero overlap with any goal produce an `Interpretive` finding.
///
/// Limitation: keyword matching does not capture semantic alignment. A session
/// about "implement the authentication system" may advance the goal
/// "ship secure login" without sharing keywords.
pub struct GoalAlignmentCheck;

impl ProsocheCheck for GoalAlignmentCheck {
    #[tracing::instrument(skip(self, state))]
    fn check<'a>(
        &'a self,
        state: &'a ProsocheState,
    ) -> Pin<Box<dyn std::future::Future<Output = Vec<Finding>> + Send + 'a>> {
        Box::pin(async move {
            let mut findings = Vec::new();

            if state.stated_goals.is_empty() || state.sessions.is_empty() {
                tracing::info!(
                    check_kind = %ProsocheCheckKind::GoalAlignment,
                    findings_count = 0,
                    "prosoche audit complete"
                );
                return findings;
            }

            let goal_terms: Vec<String> = state
                .stated_goals
                .iter()
                .flat_map(|g| extract_key_terms(g))
                .collect();

            for session in &state.sessions {
                let session_lower = session.turn_text.to_lowercase();
                let overlap = goal_terms
                    .iter()
                    .filter(|term| session_lower.contains(term.as_str()))
                    .count();

                if overlap == 0 && session.turn_count > 3 {
                    findings.push(Finding {
                        finding_id: format!("PROSOCHE-GOAL-ALIGNMENT-{}", findings.len() + 1),
                        claim: format!(
                            "Session '{}' ({} turns) has no keyword overlap with any \
                             stated goal.",
                            session.session_id, session.turn_count
                        ),
                        evidence_level: EvidenceLevel::Interpretive,
                        counter_argument:
                            "Keyword overlap is a weak proxy for semantic alignment. \
                             The session may advance goals using different terminology. \
                             Requires operator review."
                                .to_owned(),
                        source: "prosoche::GoalAlignmentCheck".to_owned(),
                        stats: FindingStats::none(),
                    });
                }
            }

            tracing::info!(
                check_kind = %ProsocheCheckKind::GoalAlignment,
                findings_count = findings.len(),
                "prosoche audit complete"
            );

            findings
        })
    }

    fn kind(&self) -> ProsocheCheckKind {
        ProsocheCheckKind::GoalAlignment
    }
}

/// Flag sessions with high error rates or only abandoned turns.
///
/// v1 thresholds:
/// - Error rate > 50% of turns: `Exploratory` finding.
/// - Zero completed sessions in the snapshot AND ≥ 5 sessions: `Interpretive` finding.
pub struct SessionQualityCheck {
    /// Error rate threshold above which a session is flagged (0.0–1.0).
    pub error_rate_threshold: f64,
    /// Minimum session length (in turns) before quality check is applied.
    ///
    /// Very short sessions are excluded to avoid flagging legitimate 1-turn interactions.
    pub min_turns: u32,
}

impl Default for SessionQualityCheck {
    fn default() -> Self {
        Self {
            error_rate_threshold: 0.5,
            min_turns: 3,
        }
    }
}

impl ProsocheCheck for SessionQualityCheck {
    #[tracing::instrument(skip(self, state))]
    fn check<'a>(
        &'a self,
        state: &'a ProsocheState,
    ) -> Pin<Box<dyn std::future::Future<Output = Vec<Finding>> + Send + 'a>> {
        Box::pin(async move {
            let mut findings = Vec::new();

            let qualified: Vec<&SessionSnapshot> = state
                .sessions
                .iter()
                .filter(|s| s.turn_count >= self.min_turns)
                .collect();

            for session in &qualified {
                if session.turn_count > 0 {
                    let error_rate = f64::from(session.error_count) / f64::from(session.turn_count);
                    if error_rate > self.error_rate_threshold {
                        findings.push(Finding {
                            finding_id: format!("PROSOCHE-SESSION-QUALITY-{}", findings.len() + 1),
                            claim: format!(
                                "Session '{}' has a {:.0}% error rate ({}/{} turns).",
                                session.session_id,
                                error_rate * 100.0,
                                session.error_count,
                                session.turn_count
                            ),
                            evidence_level: EvidenceLevel::Exploratory,
                            counter_argument:
                                "Error rate is computed from raw error responses and may include \
                                 expected tool failures. Requires operator review to distinguish \
                                 signal from noise."
                                    .to_owned(),
                            source: "prosoche::SessionQualityCheck".to_owned(),
                            stats: FindingStats::none(),
                        });
                    }
                }
            }

            let completed_count = qualified.iter().filter(|s| s.completed).count();
            if qualified.len() >= 5 && completed_count == 0 {
                findings.push(Finding {
                    finding_id: format!("PROSOCHE-SESSION-QUALITY-{}", findings.len() + 1),
                    claim: format!(
                        "None of the {} recent sessions ({} qualified) reached completion.",
                        state.sessions.len(),
                        qualified.len()
                    ),
                    evidence_level: EvidenceLevel::Interpretive,
                    counter_argument:
                        "Session completion tracking may not reflect legitimate long-running or \
                         background sessions. Requires operator review."
                            .to_owned(),
                    source: "prosoche::SessionQualityCheck".to_owned(),
                    stats: FindingStats::none(),
                });
            }

            tracing::info!(
                check_kind = %ProsocheCheckKind::SessionQuality,
                findings_count = findings.len(),
                "prosoche audit complete"
            );

            findings
        })
    }

    fn kind(&self) -> ProsocheCheckKind {
        ProsocheCheckKind::SessionQuality
    }
}

/// Detect recurring patterns in agent behavior.
///
/// v1: stub implementation. The trait shape and variant are correct; the
/// pattern detection logic requires gnomon behavioral weights and a session
/// history longer than what's available in `ProsocheState`.
///
/// # Follow-up
///
/// Full implementation tracked separately. When gnomon weights are available:
/// 1. Sample the last N session summaries.
/// 2. Run behavioural pattern detection (loop detection, avoidance bias,
///    over-confidence in tool selection).
/// 3. Emit `Speculative` findings for patterns that exceed a threshold.
pub struct InstinctPatternsCheck;

impl ProsocheCheck for InstinctPatternsCheck {
    #[tracing::instrument(skip(self, state))]
    fn check<'a>(
        &'a self,
        state: &'a ProsocheState,
    ) -> Pin<Box<dyn std::future::Future<Output = Vec<Finding>> + Send + 'a>> {
        Box::pin(async move {
            // v1 stub: gnomon behavioral weights not yet available.
            // Shape is correct; semantics mature in a follow-up issue.
            tracing::info!(
                nous_id = %state.nous_id,
                check_kind = %ProsocheCheckKind::InstinctPatterns,
                findings_count = 1,
                stub = true,
                "prosoche audit complete (stub — gnomon weights needed)"
            );

            // WHY: return a single speculative finding noting the stub state, so
            // the operator knows this check ran but has no depth yet.
            vec![Finding {
                finding_id: "PROSOCHE-INSTINCT-STUB-001".to_owned(),
                claim: "InstinctPatternsCheck is a v1 stub; no behavioral pattern data is \
                        available yet. Full detection requires gnomon weights."
                    .to_owned(),
                evidence_level: EvidenceLevel::Speculative,
                counter_argument: "This finding is itself speculative — it confirms absence of \
                                   implementation, not absence of patterns."
                    .to_owned(),
                source: "prosoche::InstinctPatternsCheck".to_owned(),
                stats: FindingStats::none(),
            }]
        })
    }

    fn kind(&self) -> ProsocheCheckKind {
        ProsocheCheckKind::InstinctPatterns
    }
}

/// Persistence backend for audit reports.
///
/// v1: writes JSON files to a directory on disk. Each audit run produces
/// a timestamped file: `prosoche-audit-<ISO8601>.json`.
pub struct AuditStorage {
    /// Directory where audit reports are written.
    pub dir: PathBuf,
}

impl AuditStorage {
    /// Create a new storage backend rooted at `dir`.
    ///
    /// The directory is created lazily on first [`AuditStorage::persist`] call.
    #[must_use]
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Persist an audit report to disk.
    ///
    /// Returns the path of the written file on success.
    pub fn persist(&self, report: &AuditReport) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(&self.dir)?;

        let ts = report.audited_at.replace([':', '.'], "-");
        let filename = format!("prosoche-audit-{ts}.json");
        let path = self.dir.join(filename);

        let json = serde_json::to_string_pretty(report)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        // WHY: std::fs::write is disallowed in daemon (daemon/clippy.toml).
        // Use File::create + write_all which is also sync but allowed.
        let mut file = std::fs::File::create(&path)?;
        file.write_all(json.as_bytes())?;

        Ok(path)
    }
}

/// Result of a single prosoche self-audit pass.
///
/// Implements [`Stamped`] so the report envelope carries provenance
/// independent of the findings it contains.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditReport {
    /// ISO 8601 timestamp when this audit ran.
    pub audited_at: String,
    /// The nous identity this audit covers.
    // kanon:ignore RUST/primitive-for-domain-id — AuditReport is a serialization envelope; nous_id is copied from the input state for provenance, not a cross-crate domain ID
    pub nous_id: String,
    /// All findings from all checks, in check order.
    pub findings: Vec<Finding>,
    /// Summary counts by check kind.
    pub check_summary: Vec<CheckSummary>,
    /// Provenance metadata (producer, schema version, counts).
    pub meta: ArtefactMeta,
}

/// Per-check summary for the audit report envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckSummary {
    /// Which check produced these findings.
    pub kind: ProsocheCheckKind,
    /// How many findings this check produced.
    pub findings_count: usize,
}

impl Stamped for AuditReport {
    fn stamp(&self) -> ArtefactMeta {
        ArtefactMeta::new(
            format!("oikonomos@{}", env!("CARGO_PKG_VERSION")),
            1,
            jiff::Timestamp::now().to_string(),
        )
        .with_count(
            "findings",
            u64::try_from(self.findings.len()).unwrap_or(u64::MAX),
        )
        .with_count(
            "checks",
            u64::try_from(self.check_summary.len()).unwrap_or(u64::MAX),
        )
    }
}

/// Runs all registered prosoche checks and persists the resulting findings.
///
/// # Usage
///
/// Build the runner with [`ProsocheAuditRunner::default_checks`], then call
/// [`ProsocheAuditRunner::run_audit`] at the heartbeat cadence. The runner is
/// wired into the existing 5-minute heartbeat via [`crate::execution`]'s
/// `SelfAudit` builtin arm — it does NOT create a new timer.
///
/// ```text
/// BuiltinTask::SelfAudit
///   └─ ProsocheAuditRunner::run_audit(&state)
///        ├─ ConsistencyCheck::check()
///        ├─ StalenessCheck::check()
///        ├─ GoalAlignmentCheck::check()
///        ├─ SessionQualityCheck::check()
///        └─ InstinctPatternsCheck::check()  (stub)
/// ```
pub struct ProsocheAuditRunner {
    /// Ordered list of checks to run.
    checks: Vec<Arc<dyn ProsocheCheck>>,
    /// Persistence backend for audit reports.
    storage: AuditStorage,
}

impl ProsocheAuditRunner {
    /// Build a runner with all five default checks and a storage backend.
    ///
    /// The `audit_dir` is the directory where [`AuditReport`] JSON files are written.
    /// A common value is `<instance_root>/data/prosoche-audits/`.
    #[must_use]
    pub fn default_checks(audit_dir: impl AsRef<Path>) -> Self {
        Self {
            checks: vec![
                Arc::new(ConsistencyCheck),
                Arc::new(StalenessCheck::default()),
                Arc::new(GoalAlignmentCheck),
                Arc::new(SessionQualityCheck::default()),
                Arc::new(InstinctPatternsCheck),
            ],
            storage: AuditStorage::new(audit_dir.as_ref()),
        }
    }

    /// Build a runner with a custom check list and storage backend.
    #[must_use]
    pub fn new(checks: Vec<Arc<dyn ProsocheCheck>>, storage: AuditStorage) -> Self {
        Self { checks, storage }
    }

    /// Run all registered checks against `state`, persist the report, and
    /// return the completed [`AuditReport`].
    ///
    /// Each check runs sequentially (they're fast heuristics). The runner
    /// does not propagate check errors — failed checks return empty finding
    /// lists and log at `WARN`.
    ///
    /// # Observability
    ///
    /// - `tracing::info!` per check with `check_kind` and `findings_count`
    ///   (emitted by each check implementation).
    /// - `tracing::info!` at audit completion with total `findings_count`.
    #[tracing::instrument(skip(self, state), fields(nous_id = %state.nous_id))]
    pub async fn run_audit(&self, state: &ProsocheState) -> AuditReport {
        let mut all_findings: Vec<Finding> = Vec::new();
        let mut check_summary: Vec<CheckSummary> = Vec::new();

        for check in &self.checks {
            let kind = check.kind();
            let findings = check.check(state).await;
            let count = findings.len();
            all_findings.extend(findings);
            check_summary.push(CheckSummary {
                kind,
                findings_count: count,
            });
        }

        let total = all_findings.len();

        // WHY: the meta stamp is built at write time, not construction time.
        let meta = ArtefactMeta::new(
            format!("oikonomos@{}", env!("CARGO_PKG_VERSION")),
            1,
            state.checked_at.clone(),
        )
        .with_count("findings", u64::try_from(total).unwrap_or(u64::MAX))
        .with_count(
            "checks",
            u64::try_from(self.checks.len()).unwrap_or(u64::MAX),
        );

        let report = AuditReport {
            audited_at: state.checked_at.clone(),
            nous_id: state.nous_id.clone(),
            findings: all_findings,
            check_summary,
            meta,
        };

        // WHY: a persist failure is logged but never fails the audit — the
        // report is still returned to the caller.
        match self.storage.persist(&report) {
            Ok(path) => {
                tracing::info!(
                    nous_id = %state.nous_id,
                    findings_count = total,
                    path = %path.display(),
                    "prosoche self-audit complete"
                );
            }
            Err(e) => {
                tracing::warn!(
                    nous_id = %state.nous_id,
                    findings_count = total,
                    error = %e,
                    "prosoche self-audit complete — report persist failed"
                );
            }
        }

        report
    }
}

#[cfg(test)]
#[path = "prosoche_audit_tests.rs"]
mod tests;
