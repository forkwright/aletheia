// kanon:ignore RUST/file-too-long — cohesive prosoche audit framework: trait + impls + report structs belong together
//! Prosoche self-audit framework: structured attention-quality checks.
//!
//! Five check types (consistency, staleness, goal alignment, session quality,
//! instinct patterns) implement [`ProsocheCheck`]; a [`ProsocheAuditRunner`]
//! runs all registered checks on demand and persists [`Finding`]s with
//! [`ArtefactMeta`] stamps and a versioned [`ProsocheReportProvenance`] envelope
//! for operator review and replay.
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
//! carries provenance independent of its contained findings. Each report also
//! carries a [`ProsocheReportProvenance`] envelope with per-check versions,
//! thresholds, sampling windows, and source snapshot hashes so the run can be
//! replayed and audited later.
//!
//! # Object safety
//!
//! `ProsocheCheck::check` returns a `Pin<Box<dyn Future>>` (same pattern as
//! [`DaemonBridge`](crate::bridge::DaemonBridge)) so the trait is object-safe
//! and `Arc<dyn ProsocheCheck>` works without `async_trait`. A synchronous
//! `metadata` method is provided for replay provenance without breaking the
//! existing trait contract.

use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use mneme::finding::{
    EvidenceLevel, EvidenceRef, Finding, FindingStats, FindingSupport, stable_hash,
};
use mneme::meta::{ArtefactMeta, Stamped};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::Instrument as _;

/// The five categories of prosoche self-audit check, one per
/// attention-quality dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
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

/// Maturity of a prosoche check, used to mark heuristic/stub/exploratory work
/// explicitly in replay provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum CheckMaturity {
    /// Check uses a well-defined algorithm but is not yet validated at scale.
    Heuristic,
    /// Check is an exploratory placeholder; semantics are not yet implemented.
    Stub,
    /// Check is intentionally lightweight and qualitative.
    Exploratory,
    /// Check is considered production-ready.
    Established,
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
    /// Recent behavioral pattern counters sampled from runtime/session history.
    ///
    /// Used by [`InstinctPatternsCheck`].
    pub behavior_patterns: Vec<BehaviorPatternSnapshot>,
    /// Recent facts for consistency and staleness checks.
    ///
    /// Each entry is `(fact_id, content, last_touched_days_ago)`.
    pub facts: Vec<FactSnapshot>,
    /// Current UTC timestamp (ISO 8601), set at audit start.
    pub checked_at: String,
}

impl ProsocheState {
    /// Deterministic hash of the query inputs (sorted ids and counts).
    ///
    /// Does not embed raw fact/session content; only the shape of the query
    /// snapshot is represented.
    #[must_use]
    pub fn source_query_hash(&self) -> String {
        let mut parts = Vec::new();
        parts.push(format!("nous_id={}", self.nous_id));
        parts.push(format!("goals={}", self.stated_goals.len()));
        parts.push(format!("behavior={}", self.behavior_patterns.len()));

        let mut facts: Vec<_> = self
            .facts
            .iter()
            .map(|f| {
                format!(
                    "fact:{}:days={}",
                    f.fact_id,
                    f.days_since_touched
                        .map_or_else(|| "unknown".to_owned(), |d| format!("{d:.6}"))
                )
            })
            .collect();
        facts.sort();
        parts.extend(facts);

        let mut sessions: Vec<_> = self
            .sessions
            .iter()
            .map(|s| {
                format!(
                    "session:{}:turns={}:errors={}:completed={}:age_days={}",
                    s.session_id,
                    s.turn_count,
                    s.error_count,
                    s.completed,
                    format_optional_days(s.session_age_days)
                )
            })
            .collect();
        sessions.sort();
        parts.extend(sessions);

        let mut behavior: Vec<_> = self
            .behavior_patterns
            .iter()
            .map(|b| {
                format!(
                    "behavior:{}:tools={}:tool_errors={}:repeats={}:no_progress={}:avoidance={}:confidence={}",
                    b.session_id,
                    b.tool_call_count,
                    b.tool_error_count,
                    b.repeated_action_count,
                    b.no_progress_turns,
                    b.avoidance_markers,
                    b.confidence_claims
                )
            })
            .collect();
        behavior.sort();
        parts.extend(behavior);

        stable_hash(&parts.join("\n"))
    }

    /// Deterministic hash of the full snapshot, including content hashes.
    ///
    /// Embedding content hashes (not the content itself) lets a later auditor
    /// confirm whether the same underlying facts and sessions were used.
    #[must_use]
    pub fn source_snapshot_hash(&self) -> String {
        let mut parts = Vec::new();
        parts.push(format!("nous_id={}", self.nous_id));
        parts.push(format!("goals={}", self.stated_goals.len()));
        parts.push(format!("behavior={}", self.behavior_patterns.len()));

        let mut facts: Vec<_> = self
            .facts
            .iter()
            .map(|f| {
                format!(
                    "fact:{}:days={}:content_hash={}",
                    f.fact_id,
                    f.days_since_touched
                        .map_or_else(|| "unknown".to_owned(), |d| format!("{d:.6}")),
                    stable_hash(&f.content)
                )
            })
            .collect();
        facts.sort();
        parts.extend(facts);

        let mut sessions: Vec<_> = self
            .sessions
            .iter()
            .map(|s| {
                format!(
                    "session:{}:turns={}:errors={}:completed={}:age_days={}:turn_hash={}",
                    s.session_id,
                    s.turn_count,
                    s.error_count,
                    s.completed,
                    format_optional_days(s.session_age_days),
                    stable_hash(&s.turn_text)
                )
            })
            .collect();
        sessions.sort();
        parts.extend(sessions);

        let mut behavior: Vec<_> = self
            .behavior_patterns
            .iter()
            .map(|b| {
                format!(
                    "behavior:{}:tools={}:tool_errors={}:repeats={}:no_progress={}:avoidance={}:confidence={}",
                    b.session_id,
                    b.tool_call_count,
                    b.tool_error_count,
                    b.repeated_action_count,
                    b.no_progress_turns,
                    b.avoidance_markers,
                    b.confidence_claims
                )
            })
            .collect();
        behavior.sort();
        parts.extend(behavior);

        stable_hash(&parts.join("\n"))
    }
}

/// A minimal fact snapshot for audit checks.
#[derive(Debug, Clone)]
// kanon:ignore TOPOLOGY/shallow-struct WHY: ephemeral audit input assembled by state collectors; checks intentionally read fields directly
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
// kanon:ignore TOPOLOGY/shallow-struct WHY(#4721): session staleness input must expose age and counts without binding checks to a store type
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
    /// How many days old the session is at audit time.
    ///
    /// A value of `None` means session age is unknown.
    pub session_age_days: Option<f64>,
    /// Combined text of all user turns in this session.
    ///
    /// Used for goal-alignment keyword matching. Only hashes of this value are
    /// persisted in durable reports.
    pub turn_text: String,
}

fn format_optional_days(days: Option<f64>) -> String {
    days.map_or_else(|| "unknown".to_owned(), |d| format!("{d:.6}"))
}

/// Behavioral counters for one recent session.
#[derive(Debug, Clone, Default)]
// kanon:ignore TOPOLOGY/shallow-struct WHY: behavior counters are ephemeral audit inputs shared across instinct-pattern checks
pub struct BehaviorPatternSnapshot {
    /// Session identifier that owns the behavior sample.
    // kanon:ignore RUST/primitive-for-domain-id — BehaviorPatternSnapshot is an ephemeral audit input keyed by external session ids
    pub session_id: String,
    /// Tool calls attempted during the sampled window.
    pub tool_call_count: u32,
    /// Tool calls that returned errors during the sampled window.
    pub tool_error_count: u32,
    /// Repeated actions or near-identical attempts observed in the window.
    pub repeated_action_count: u32,
    /// Turns explicitly marked as stuck, looping, or making no progress.
    pub no_progress_turns: u32,
    /// Markers indicating deferral, skipping, or avoidance of the stated task.
    pub avoidance_markers: u32,
    /// High-confidence assertions or tool-selection claims.
    pub confidence_claims: u32,
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
/// - Checks SHOULD NOT panic. The runner records task failures as `CheckFailure`
///   entries and continues the remaining checks.
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

    /// Replay provenance for this check.
    ///
    /// Default implementation provides a generic envelope; concrete checks
    /// should override to advertise their version, thresholds, and maturity.
    fn metadata(&self, state: &ProsocheState) -> CheckProvenance {
        CheckProvenance {
            kind: self.kind(),
            version: "0.0.0".to_owned(),
            maturity: CheckMaturity::Exploratory,
            thresholds: Value::Null,
            sampling_window: None,
            source_query_hash: state.source_query_hash(),
        }
    }
}

/// Per-check replay provenance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckProvenance {
    /// Which check this provenance describes.
    pub kind: ProsocheCheckKind,
    /// Semantic version of the check implementation.
    pub version: String,
    /// Maturity of the check implementation.
    pub maturity: CheckMaturity,
    /// Threshold parameters that influenced the check, if any.
    pub thresholds: Value,
    /// Human-readable sampling window description, if applicable.
    pub sampling_window: Option<String>,
    /// Hash of the inputs relevant to this check.
    pub source_query_hash: String,
}

/// Record of a check that failed during the audit run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckFailure {
    /// Which check failed.
    pub kind: ProsocheCheckKind,
    /// Human-readable failure reason.
    pub reason: String,
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

impl ConsistencyCheck {
    fn query_hash(state: &ProsocheState) -> String {
        let mut parts: Vec<String> = state
            .facts
            .iter()
            .map(|f| format!("{}:{}", f.fact_id, stable_hash(&f.content)))
            .collect();
        parts.sort();
        stable_hash(&parts.join("\n"))
    }
}

// kanon:ignore ARCHITECTURE/trait-impl-colocation WHY: prosoche checks are the daemon-local implementations registered by this module
impl ProsocheCheck for ConsistencyCheck {
    #[tracing::instrument(skip(self, state))]
    fn check<'a>(
        &'a self,
        state: &'a ProsocheState,
    ) -> Pin<Box<dyn std::future::Future<Output = Vec<Finding>> + Send + 'a>> {
        Box::pin(async move {
            let mut findings = Vec::new();

            let normalised: Vec<(String, String, Vec<String>, String)> = state
                .facts
                .iter()
                .map(|f| {
                    let terms = extract_key_terms(&f.content);
                    let content_hash = stable_hash(&f.content);
                    (f.fact_id.clone(), f.content.clone(), terms, content_hash)
                })
                .collect();

            let fact_count = normalised.len();
            let total_pairs = fact_count.saturating_mul(fact_count.saturating_sub(1)) / 2;
            let query_hash = Self::query_hash(state);

            for (i, (id_a, content_a, terms_a, hash_a)) in normalised.iter().enumerate() {
                for (id_b, content_b, _, hash_b) in normalised.get(i + 1..).unwrap_or_default() {
                    for term in terms_a {
                        let negated = format!("not {term}");
                        let negated_alt = format!("never {term}");
                        let content_b_lower = content_b.to_lowercase();
                        if content_b_lower.contains(&negated)
                            || content_b_lower.contains(&negated_alt)
                        {
                            let rate = if total_pairs == 0 {
                                None
                            } else {
                                Some(
                                    1.0 / f64::from(u32::try_from(total_pairs).unwrap_or(u32::MAX)),
                                )
                            };
                            findings.push(Finding {
                                finding_id: format!("PROSOCHE-CONSISTENCY-{}", findings.len() + 1),
                                claim: format!(
                                    "Fact '{id_a}' and fact '{id_b}' matched the term-negation \
                                     contradiction heuristic."
                                ),
                                evidence_level: EvidenceLevel::Exploratory,
                                counter_argument:
                                    "Term-negation heuristic; may be false positive on \
                                     nuanced phrasing. Requires human review."
                                        .to_owned(),
                                source: "prosoche::ConsistencyCheck".to_owned(),
                                stats: FindingStats {
                                    p_adjusted: None,
                                    effect_metric: Some("contradiction_rate".to_owned()),
                                    effect_value: None,
                                    ci: None,
                                    sample_sizes: Some([total_pairs, fact_count]),
                                    rate,
                                    support: Some(FindingSupport {
                                        evidence_refs: vec![
                                            EvidenceRef::Fact {
                                                fact_id: id_a.clone(),
                                                content_hash: hash_a.clone(),
                                            },
                                            EvidenceRef::Fact {
                                                fact_id: id_b.clone(),
                                                content_hash: hash_b.clone(),
                                            },
                                            EvidenceRef::Query {
                                                query_hash: query_hash.clone(),
                                            },
                                        ],
                                        is_stub: false,
                                        is_heuristic: true,
                                    }),
                                },
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
                            let rate = if total_pairs == 0 {
                                None
                            } else {
                                Some(
                                    1.0 / f64::from(u32::try_from(total_pairs).unwrap_or(u32::MAX)),
                                )
                            };
                            findings.push(Finding {
                                finding_id: format!("PROSOCHE-CONSISTENCY-{}", findings.len() + 1),
                                claim: format!(
                                    "Fact '{id_a}' and fact '{id_b}' matched the term-negation \
                                     contradiction heuristic."
                                ),
                                evidence_level: EvidenceLevel::Exploratory,
                                counter_argument:
                                    "Term-negation heuristic; may be false positive on \
                                     nuanced phrasing. Requires human review."
                                        .to_owned(),
                                source: "prosoche::ConsistencyCheck".to_owned(),
                                stats: FindingStats {
                                    p_adjusted: None,
                                    effect_metric: Some("contradiction_rate".to_owned()),
                                    effect_value: None,
                                    ci: None,
                                    sample_sizes: Some([total_pairs, fact_count]),
                                    rate,
                                    support: Some(FindingSupport {
                                        evidence_refs: vec![
                                            EvidenceRef::Fact {
                                                fact_id: id_a.clone(),
                                                content_hash: hash_a.clone(),
                                            },
                                            EvidenceRef::Fact {
                                                fact_id: id_b.clone(),
                                                content_hash: hash_b.clone(),
                                            },
                                            EvidenceRef::Query {
                                                query_hash: query_hash.clone(),
                                            },
                                        ],
                                        is_stub: false,
                                        is_heuristic: true,
                                    }),
                                },
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

    fn metadata(&self, state: &ProsocheState) -> CheckProvenance {
        CheckProvenance {
            kind: self.kind(),
            version: "1.0.0".to_owned(),
            maturity: CheckMaturity::Heuristic,
            thresholds: Value::Null,
            sampling_window: None,
            source_query_hash: Self::query_hash(state),
        }
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
/// - Incomplete sessions older than 14 days with > 10 turns: `Interpretive` finding.
pub struct StalenessCheck {
    /// Number of days after which an unaccessed fact is considered stale.
    pub fact_stale_days: f64,
    /// Number of days after which an incomplete session is considered stale.
    pub session_stale_days: f64,
    /// Minimum unfinished turns before session staleness is evaluated.
    pub incomplete_session_turn_threshold: u32,
}

impl Default for StalenessCheck {
    fn default() -> Self {
        Self {
            fact_stale_days: 90.0,
            session_stale_days: 14.0,
            incomplete_session_turn_threshold: 10,
        }
    }
}

impl StalenessCheck {
    fn query_hash(state: &ProsocheState) -> String {
        let mut parts: Vec<String> = state
            .facts
            .iter()
            .map(|f| {
                format!(
                    "fact:{}:days={}",
                    f.fact_id,
                    f.days_since_touched
                        .map_or_else(|| "unknown".to_owned(), |d| format!("{d:.6}"))
                )
            })
            .collect();
        parts.sort();

        let mut sessions: Vec<String> = state
            .sessions
            .iter()
            .map(|s| {
                format!(
                    "session:{}:turns={}:completed={}:age_days={}",
                    s.session_id,
                    s.turn_count,
                    s.completed,
                    format_optional_days(s.session_age_days)
                )
            })
            .collect();
        sessions.sort();
        parts.extend(sessions);

        stable_hash(&parts.join("\n"))
    }
}

// kanon:ignore ARCHITECTURE/trait-impl-colocation WHY: prosoche checks are the daemon-local implementations registered by this module
impl ProsocheCheck for StalenessCheck {
    #[tracing::instrument(skip(self, state))]
    fn check<'a>(
        &'a self,
        state: &'a ProsocheState,
    ) -> Pin<Box<dyn std::future::Future<Output = Vec<Finding>> + Send + 'a>> {
        Box::pin(async move {
            let mut findings = Vec::new();

            let facts_scanned = state.facts.len();
            let sessions_scanned = state.sessions.len();
            let query_hash = Self::query_hash(state);

            for fact in &state.facts {
                if let Some(days) = fact.days_since_touched
                    && days > self.fact_stale_days
                {
                    let rate = if facts_scanned == 0 {
                        None
                    } else {
                        Some(1.0 / f64::from(u32::try_from(facts_scanned).unwrap_or(u32::MAX)))
                    };
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
                        stats: FindingStats {
                            p_adjusted: None,
                            effect_metric: Some("stale_fact_rate".to_owned()),
                            effect_value: None,
                            ci: None,
                            sample_sizes: Some([1, facts_scanned]),
                            rate,
                            support: Some(FindingSupport {
                                evidence_refs: vec![
                                    EvidenceRef::Fact {
                                        fact_id: fact.fact_id.clone(),
                                        content_hash: stable_hash(&fact.content),
                                    },
                                    EvidenceRef::Query {
                                        query_hash: query_hash.clone(),
                                    },
                                ],
                                is_stub: false,
                                is_heuristic: true,
                            }),
                        },
                    });
                }
            }

            for session in &state.sessions {
                if let Some(session_age_days) = session.session_age_days
                    && !session.completed
                    && session.turn_count > self.incomplete_session_turn_threshold
                    && session_age_days > self.session_stale_days
                {
                    let rate = if sessions_scanned == 0 {
                        None
                    } else {
                        Some(1.0 / f64::from(u32::try_from(sessions_scanned).unwrap_or(u32::MAX)))
                    };
                    findings.push(Finding {
                        finding_id: format!("PROSOCHE-STALENESS-SESSION-{}", findings.len() + 1),
                        claim: format!(
                            "Session '{}' is {session_age_days:.0} days old, has {} turns, \
                             and was never completed (thresholds: >{:.0} days, >{} turns).",
                            session.session_id,
                            session.turn_count,
                            self.session_stale_days,
                            self.incomplete_session_turn_threshold
                        ),
                        evidence_level: EvidenceLevel::Interpretive,
                        counter_argument:
                            "Old incomplete sessions may reflect legitimate open-ended work. \
                             Requires operator review before archival or follow-up."
                                .to_owned(),
                        source: "prosoche::StalenessCheck".to_owned(),
                        stats: FindingStats {
                            p_adjusted: None,
                            effect_metric: Some("stale_incomplete_session_rate".to_owned()),
                            effect_value: None,
                            ci: None,
                            sample_sizes: Some([1, sessions_scanned]),
                            rate,
                            support: Some(FindingSupport {
                                evidence_refs: vec![
                                    EvidenceRef::Session {
                                        session_id: session.session_id.clone(),
                                        turn_hash: stable_hash(&session.turn_text),
                                    },
                                    EvidenceRef::Query {
                                        query_hash: query_hash.clone(),
                                    },
                                ],
                                is_stub: false,
                                is_heuristic: true,
                            }),
                        },
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

    fn metadata(&self, state: &ProsocheState) -> CheckProvenance {
        CheckProvenance {
            kind: self.kind(),
            version: "1.0.0".to_owned(),
            maturity: CheckMaturity::Heuristic,
            thresholds: serde_json::json!({
                "fact_stale_days": self.fact_stale_days,
                "session_stale_days": self.session_stale_days,
                "incomplete_session_turn_threshold": self.incomplete_session_turn_threshold,
            }),
            sampling_window: None,
            source_query_hash: Self::query_hash(state),
        }
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

impl GoalAlignmentCheck {
    fn query_hash(state: &ProsocheState) -> String {
        let mut parts: Vec<String> = state
            .stated_goals
            .iter()
            .map(|g| format!("goal:{}", stable_hash(g)))
            .collect();
        parts.sort();

        let mut sessions: Vec<String> = state
            .sessions
            .iter()
            .map(|s| {
                format!(
                    "session:{}:turn_hash={}",
                    s.session_id,
                    stable_hash(&s.turn_text)
                )
            })
            .collect();
        sessions.sort();
        parts.extend(sessions);

        stable_hash(&parts.join("\n"))
    }
}

// kanon:ignore ARCHITECTURE/trait-impl-colocation WHY: prosoche checks are the daemon-local implementations registered by this module
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
            let goal_terms_count = goal_terms.len();
            let sessions_scanned = state.sessions.len();
            let qualified: Vec<&SessionSnapshot> =
                state.sessions.iter().filter(|s| s.turn_count > 3).collect();
            let qualified_count = qualified.len();
            let query_hash = Self::query_hash(state);

            for session in &qualified {
                let session_lower = session.turn_text.to_lowercase();
                let overlap = goal_terms
                    .iter()
                    .filter(|term| session_lower.contains(term.as_str()))
                    .count();

                if overlap == 0 {
                    let rate = if qualified_count == 0 {
                        None
                    } else {
                        Some(1.0 / f64::from(u32::try_from(qualified_count).unwrap_or(u32::MAX)))
                    };
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
                        stats: FindingStats {
                            p_adjusted: None,
                            effect_metric: Some("goal_misalignment_rate".to_owned()),
                            effect_value: None,
                            ci: None,
                            sample_sizes: Some([1, qualified_count]),
                            rate,
                            support: Some(FindingSupport {
                                evidence_refs: vec![
                                    EvidenceRef::Session {
                                        session_id: session.session_id.clone(),
                                        turn_hash: stable_hash(&session.turn_text),
                                    },
                                    EvidenceRef::Query {
                                        query_hash: query_hash.clone(),
                                    },
                                ],
                                is_stub: false,
                                is_heuristic: true,
                            }),
                        },
                    });
                }
            }

            tracing::info!(
                check_kind = %ProsocheCheckKind::GoalAlignment,
                findings_count = findings.len(),
                goal_terms_count,
                sessions_scanned,
                qualified_count,
                "prosoche audit complete"
            );

            findings
        })
    }

    fn kind(&self) -> ProsocheCheckKind {
        ProsocheCheckKind::GoalAlignment
    }

    fn metadata(&self, state: &ProsocheState) -> CheckProvenance {
        CheckProvenance {
            kind: self.kind(),
            version: "1.0.0".to_owned(),
            maturity: CheckMaturity::Heuristic,
            thresholds: serde_json::json!({ "goal_terms_count": state.stated_goals.len() }),
            sampling_window: None,
            source_query_hash: Self::query_hash(state),
        }
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

impl SessionQualityCheck {
    fn query_hash(state: &ProsocheState) -> String {
        let mut parts: Vec<String> = state
            .sessions
            .iter()
            .map(|s| {
                format!(
                    "session:{}:turns={}:errors={}:completed={}",
                    s.session_id, s.turn_count, s.error_count, s.completed
                )
            })
            .collect();
        parts.sort();
        stable_hash(&parts.join("\n"))
    }
}

// kanon:ignore ARCHITECTURE/trait-impl-colocation WHY: prosoche checks are the daemon-local implementations registered by this module
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
            let qualified_count = qualified.len();
            let query_hash = Self::query_hash(state);

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
                            stats: FindingStats {
                                p_adjusted: None,
                                effect_metric: Some("error_rate".to_owned()),
                                effect_value: Some(error_rate),
                                ci: None,
                                sample_sizes: Some([
                                    usize::try_from(session.error_count).unwrap_or(usize::MAX),
                                    usize::try_from(session.turn_count).unwrap_or(usize::MAX),
                                ]),
                                rate: Some(error_rate),
                                support: Some(FindingSupport {
                                    evidence_refs: vec![
                                        EvidenceRef::Session {
                                            session_id: session.session_id.clone(),
                                            turn_hash: stable_hash(&session.turn_text),
                                        },
                                        EvidenceRef::Query {
                                            query_hash: query_hash.clone(),
                                        },
                                    ],
                                    is_stub: false,
                                    is_heuristic: true,
                                }),
                            },
                        });
                    }
                }
            }

            let completed_count = qualified.iter().filter(|s| s.completed).count();
            if qualified.len() >= 5 && completed_count == 0 {
                let rate = if qualified_count == 0 {
                    None
                } else {
                    Some(
                        f64::from(u32::try_from(completed_count).unwrap_or(u32::MAX))
                            / f64::from(u32::try_from(qualified_count).unwrap_or(u32::MAX)),
                    )
                };
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
                    stats: FindingStats {
                        p_adjusted: None,
                        effect_metric: Some("completion_rate".to_owned()),
                        effect_value: rate,
                        ci: None,
                        sample_sizes: Some([completed_count, qualified_count]),
                        rate,
                        support: Some(FindingSupport {
                            evidence_refs: vec![EvidenceRef::Query {
                                query_hash: query_hash.clone(),
                            }],
                            is_stub: false,
                            is_heuristic: true,
                        }),
                    },
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

    fn metadata(&self, state: &ProsocheState) -> CheckProvenance {
        CheckProvenance {
            kind: self.kind(),
            version: "1.0.0".to_owned(),
            maturity: CheckMaturity::Heuristic,
            thresholds: serde_json::json!({
                "error_rate_threshold": self.error_rate_threshold,
                "min_turns": self.min_turns,
            }),
            sampling_window: None,
            source_query_hash: Self::query_hash(state),
        }
    }
}

/// Detect recurring patterns in agent behavior.
///
/// v1 heuristic: combines typed behavior counters with lightweight text
/// markers inferred from recent session turn text. Typed counters carry
/// `Exploratory` evidence; text-only fallback findings remain `Speculative`.
pub struct InstinctPatternsCheck;

impl InstinctPatternsCheck {
    const MIN_TURNS: u32 = 4;
    const LOOP_MARKER_THRESHOLD: u32 = 3;
    const TOOL_ERROR_RATE_THRESHOLD: f64 = 0.5;
    const AVOIDANCE_MARKER_THRESHOLD: u32 = 3;
    const CONFIDENCE_MARKER_THRESHOLD: u32 = 3;

    fn query_hash(state: &ProsocheState) -> String {
        let mut parts: Vec<String> = state
            .sessions
            .iter()
            .map(|s| {
                format!(
                    "session:{}:turns={}:errors={}:completed={}:turn_hash={}",
                    s.session_id,
                    s.turn_count,
                    s.error_count,
                    s.completed,
                    stable_hash(&s.turn_text)
                )
            })
            .collect();
        parts.extend(state.behavior_patterns.iter().map(|b| {
            format!(
                "behavior:{}:tools={}:tool_errors={}:repeats={}:no_progress={}:avoidance={}:confidence={}",
                b.session_id,
                b.tool_call_count,
                b.tool_error_count,
                b.repeated_action_count,
                b.no_progress_turns,
                b.avoidance_markers,
                b.confidence_claims
            )
        }));
        parts.sort();
        stable_hash(&parts.join("\n"))
    }
}

#[derive(Debug)]
struct BehaviorSample {
    session_id: String,
    turn_count: u32,
    completed: bool,
    turn_hash: String,
    tool_call_count: u32,
    tool_error_count: u32,
    repeated_action_count: u32,
    no_progress_turns: u32,
    avoidance_markers: u32,
    confidence_claims: u32,
    inferred_from_text: bool,
}

fn behavior_samples(state: &ProsocheState) -> Vec<BehaviorSample> {
    let mut samples = Vec::new();

    for behavior in &state.behavior_patterns {
        let session = state
            .sessions
            .iter()
            .find(|session| session.session_id == behavior.session_id);
        samples.push(BehaviorSample {
            session_id: behavior.session_id.clone(),
            turn_count: session.map_or(0, |session| session.turn_count),
            completed: session.is_some_and(|session| session.completed),
            turn_hash: session.map_or_else(
                || stable_hash(&behavior.session_id),
                |session| stable_hash(&session.turn_text),
            ),
            tool_call_count: behavior.tool_call_count,
            tool_error_count: behavior.tool_error_count,
            repeated_action_count: behavior.repeated_action_count,
            no_progress_turns: behavior.no_progress_turns,
            avoidance_markers: behavior.avoidance_markers,
            confidence_claims: behavior.confidence_claims,
            inferred_from_text: false,
        });
    }

    for session in &state.sessions {
        if state
            .behavior_patterns
            .iter()
            .any(|behavior| behavior.session_id == session.session_id)
        {
            continue;
        }
        samples.push(infer_behavior_sample(session));
    }

    samples
}

fn infer_behavior_sample(session: &SessionSnapshot) -> BehaviorSample {
    let lower = session.turn_text.to_lowercase();
    let tool_call_count = count_markers(&lower, &["tool", "command", "search", "read", "write"]);
    let tool_error_count =
        count_markers(&lower, &["error", "failed", "failure", "timeout", "panic"])
            .max(session.error_count);

    BehaviorSample {
        session_id: session.session_id.clone(),
        turn_count: session.turn_count,
        completed: session.completed,
        turn_hash: stable_hash(&session.turn_text),
        tool_call_count,
        tool_error_count,
        repeated_action_count: count_markers(&lower, &["again", "retry", "same error"]),
        no_progress_turns: count_markers(
            &lower,
            &["no progress", "stuck", "still failing", "loop", "blocked"],
        ),
        avoidance_markers: count_markers(
            &lower,
            &["defer", "later", "skip", "avoid", "not necessary", "cannot"],
        ),
        confidence_claims: count_markers(
            &lower,
            &[
                "definitely",
                "guaranteed",
                "certain",
                "obviously",
                "will work",
            ],
        ),
        inferred_from_text: true,
    }
}

fn count_markers(haystack: &str, markers: &[&str]) -> u32 {
    let count = markers
        .iter()
        .map(|marker| haystack.matches(marker).count())
        .sum::<usize>();
    u32::try_from(count).unwrap_or(u32::MAX)
}

fn sample_evidence_level(sample: &BehaviorSample) -> EvidenceLevel {
    if sample.inferred_from_text {
        EvidenceLevel::Speculative
    } else {
        EvidenceLevel::Exploratory
    }
}

fn session_support(sample: &BehaviorSample, query_hash: &str) -> FindingSupport {
    FindingSupport {
        evidence_refs: vec![
            EvidenceRef::Session {
                session_id: sample.session_id.clone(),
                turn_hash: sample.turn_hash.clone(),
            },
            EvidenceRef::Query {
                query_hash: query_hash.to_owned(),
            },
        ],
        is_stub: false,
        is_heuristic: true,
    }
}

// kanon:ignore ARCHITECTURE/trait-impl-colocation WHY: prosoche checks are the daemon-local implementations registered by this module
impl ProsocheCheck for InstinctPatternsCheck {
    #[tracing::instrument(skip(self, state))]
    fn check<'a>(
        &'a self,
        state: &'a ProsocheState,
    ) -> Pin<Box<dyn std::future::Future<Output = Vec<Finding>> + Send + 'a>> {
        Box::pin(async move {
            let query_hash = Self::query_hash(state);
            let samples: Vec<_> = behavior_samples(state)
                .into_iter()
                .filter(|sample| {
                    sample.turn_count >= Self::MIN_TURNS
                        || sample.tool_call_count > 0
                        || sample.no_progress_turns > 0
                        || sample.avoidance_markers > 0
                        || sample.confidence_claims > 0
                })
                .collect();
            let mut findings = Vec::new();

            for sample in &samples {
                let loop_markers = sample
                    .no_progress_turns
                    .saturating_add(sample.repeated_action_count);
                if loop_markers >= Self::LOOP_MARKER_THRESHOLD {
                    let denominator = sample.turn_count.max(1);
                    let rate = f64::from(loop_markers) / f64::from(denominator);
                    findings.push(Finding {
                        finding_id: format!("PROSOCHE-INSTINCT-LOOP-{}", findings.len() + 1),
                        claim: format!(
                            "Session '{}' shows {} loop/no-progress markers across {} turns.",
                            sample.session_id, loop_markers, sample.turn_count
                        ),
                        evidence_level: sample_evidence_level(sample),
                        counter_argument:
                            "Repeated-attempt markers can be legitimate during hard debugging. \
                             Confirm whether the session changed strategy before treating this \
                             as behavioral drift."
                                .to_owned(),
                        source: "prosoche::InstinctPatternsCheck".to_owned(),
                        stats: FindingStats {
                            p_adjusted: None,
                            effect_metric: Some("loop_marker_rate".to_owned()),
                            effect_value: Some(rate),
                            ci: None,
                            sample_sizes: Some([
                                usize::try_from(loop_markers).unwrap_or(usize::MAX),
                                usize::try_from(denominator).unwrap_or(usize::MAX),
                            ]),
                            rate: Some(rate),
                            support: Some(session_support(sample, &query_hash)),
                        },
                    });
                }

                if sample.tool_call_count >= 3 {
                    let tool_error_rate =
                        f64::from(sample.tool_error_count) / f64::from(sample.tool_call_count);
                    if tool_error_rate >= Self::TOOL_ERROR_RATE_THRESHOLD {
                        findings.push(Finding {
                            finding_id: format!("PROSOCHE-INSTINCT-TOOLS-{}", findings.len() + 1),
                            claim: format!(
                                "Session '{}' has a {:.0}% tool-error rate ({}/{} tool calls).",
                                sample.session_id,
                                tool_error_rate * 100.0,
                                sample.tool_error_count,
                                sample.tool_call_count
                            ),
                            evidence_level: sample_evidence_level(sample),
                            counter_argument:
                                "Tool failures may come from unavailable dependencies or expected \
                                 negative probes. Review the underlying calls before attributing \
                                 the pattern to agent behavior."
                                    .to_owned(),
                            source: "prosoche::InstinctPatternsCheck".to_owned(),
                            stats: FindingStats {
                                p_adjusted: None,
                                effect_metric: Some("tool_error_rate".to_owned()),
                                effect_value: Some(tool_error_rate),
                                ci: None,
                                sample_sizes: Some([
                                    usize::try_from(sample.tool_error_count).unwrap_or(usize::MAX),
                                    usize::try_from(sample.tool_call_count).unwrap_or(usize::MAX),
                                ]),
                                rate: Some(tool_error_rate),
                                support: Some(session_support(sample, &query_hash)),
                            },
                        });
                    }
                }

                if !sample.completed && sample.avoidance_markers >= Self::AVOIDANCE_MARKER_THRESHOLD
                {
                    let denominator = sample.turn_count.max(1);
                    let rate = f64::from(sample.avoidance_markers) / f64::from(denominator);
                    findings.push(Finding {
                        finding_id: format!("PROSOCHE-INSTINCT-AVOIDANCE-{}", findings.len() + 1),
                        claim: format!(
                            "Session '{}' has {} avoidance markers and did not complete.",
                            sample.session_id, sample.avoidance_markers
                        ),
                        evidence_level: sample_evidence_level(sample),
                        counter_argument:
                            "Deferral language can reflect correct prioritization or missing \
                             authority. Treat this as a review prompt, not proof of avoidance."
                                .to_owned(),
                        source: "prosoche::InstinctPatternsCheck".to_owned(),
                        stats: FindingStats {
                            p_adjusted: None,
                            effect_metric: Some("avoidance_marker_rate".to_owned()),
                            effect_value: Some(rate),
                            ci: None,
                            sample_sizes: Some([
                                usize::try_from(sample.avoidance_markers).unwrap_or(usize::MAX),
                                usize::try_from(denominator).unwrap_or(usize::MAX),
                            ]),
                            rate: Some(rate),
                            support: Some(session_support(sample, &query_hash)),
                        },
                    });
                }

                if sample.confidence_claims >= Self::CONFIDENCE_MARKER_THRESHOLD
                    && sample.tool_error_count > 0
                {
                    let denominator = sample.turn_count.max(1);
                    let rate = f64::from(sample.confidence_claims) / f64::from(denominator);
                    findings.push(Finding {
                        finding_id: format!("PROSOCHE-INSTINCT-CONFIDENCE-{}", findings.len() + 1),
                        claim: format!(
                            "Session '{}' pairs {} confidence markers with {} tool errors.",
                            sample.session_id, sample.confidence_claims, sample.tool_error_count
                        ),
                        evidence_level: sample_evidence_level(sample),
                        counter_argument:
                            "Confidence markers may appear in quoted text or user instructions. \
                             Confirm speaker attribution before treating this as over-confidence."
                                .to_owned(),
                        source: "prosoche::InstinctPatternsCheck".to_owned(),
                        stats: FindingStats {
                            p_adjusted: None,
                            effect_metric: Some("confidence_marker_rate".to_owned()),
                            effect_value: Some(rate),
                            ci: None,
                            sample_sizes: Some([
                                usize::try_from(sample.confidence_claims).unwrap_or(usize::MAX),
                                usize::try_from(denominator).unwrap_or(usize::MAX),
                            ]),
                            rate: Some(rate),
                            support: Some(session_support(sample, &query_hash)),
                        },
                    });
                }
            }

            tracing::info!(
                nous_id = %state.nous_id,
                check_kind = %ProsocheCheckKind::InstinctPatterns,
                findings_count = findings.len(),
                samples = samples.len(),
                "prosoche audit complete"
            );

            findings
        })
    }

    fn kind(&self) -> ProsocheCheckKind {
        ProsocheCheckKind::InstinctPatterns
    }

    fn metadata(&self, state: &ProsocheState) -> CheckProvenance {
        CheckProvenance {
            kind: self.kind(),
            version: "1.0.0".to_owned(),
            maturity: CheckMaturity::Heuristic,
            thresholds: serde_json::json!({
                "min_turns": Self::MIN_TURNS,
                "loop_marker_threshold": Self::LOOP_MARKER_THRESHOLD,
                "tool_error_rate_threshold": Self::TOOL_ERROR_RATE_THRESHOLD,
                "avoidance_marker_threshold": Self::AVOIDANCE_MARKER_THRESHOLD,
                "confidence_marker_threshold": Self::CONFIDENCE_MARKER_THRESHOLD,
            }),
            sampling_window: Some("recent ProsocheState sessions and behavior counters".to_owned()),
            source_query_hash: Self::query_hash(state),
        }
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
    pub async fn persist(&self, report: &AuditReport) -> std::io::Result<PathBuf> {
        tokio::fs::create_dir_all(&self.dir).await?;

        let ts = report.audited_at.replace([':', '.'], "-");
        let filename = format!("prosoche-audit-{ts}.json");
        let path = self.dir.join(filename);

        let json = serde_json::to_string_pretty(report)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        // WHY: use tokio::fs so the audit JSON write does not block the
        // executor thread; this replaces the previous sync two-step workaround.
        tokio::fs::write(&path, json).await?;

        Ok(path)
    }
}

/// Versioned provenance envelope for a prosoche audit report.
///
/// Captures everything needed to replay or audit the run: check versions,
/// thresholds, sampling windows, code/build identifiers, config hash, and
/// hashes of the source query/snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProsocheReportProvenance {
    /// Report provenance schema version.
    pub report_version: String,
    /// Daemon version that produced the report.
    pub daemon_version: String,
    /// Source code SHA, if available at build time.
    pub code_sha: Option<String>,
    /// Build SHA, if available at build time.
    pub build_sha: Option<String>,
    /// Hash of the daemon config active at audit time, if available.
    pub config_hash: Option<String>,
    /// Hash of the source query (sorted ids and counts).
    pub source_query_hash: String,
    /// Hash of the full source snapshot (including content hashes).
    pub source_snapshot_hash: String,
    /// Per-check replay provenance.
    pub checks: Vec<CheckProvenance>,
    /// Checks that failed during the run.
    pub check_failures: Vec<CheckFailure>,
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
    /// Versioned replay provenance envelope.
    #[serde(default)]
    pub provenance: Option<ProsocheReportProvenance>,
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

/// Result of running and persisting a prosoche self-audit pass.
///
/// The computed [`AuditReport`] is always produced, even when persistence fails.
/// The outcome records whether the report was written to disk (`persisted_path`)
/// or why the write failed (`last_persist_error`). Only one of those two fields
/// will be set for any given run.
#[derive(Debug, Clone)]
pub struct ProsocheAuditOutcome {
    /// The computed audit report, independent of persistence success or failure.
    pub report: AuditReport,
    /// Path where the report was persisted, when persistence succeeded.
    pub persisted_path: Option<PathBuf>,
    /// Human-readable persistence error, when persistence failed.
    pub last_persist_error: Option<String>,
}

impl Deref for ProsocheAuditOutcome {
    type Target = AuditReport;

    fn deref(&self) -> &Self::Target {
        &self.report
    }
}

impl DerefMut for ProsocheAuditOutcome {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.report
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
///        └─ InstinctPatternsCheck::check()
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
    /// return a [`ProsocheAuditOutcome`] containing the completed [`AuditReport`].
    ///
    /// Each check runs sequentially (they're fast heuristics). The runner
    /// captures check panics as [`CheckFailure`] records rather than failing
    /// the whole audit. A persistence failure is captured in the outcome and
    /// never fails the audit.
    ///
    /// # Observability
    ///
    /// - `tracing::info!` per check with `check_kind` and `findings_count`
    ///   (emitted by each check implementation).
    /// - `tracing::info!` at audit completion with total `findings_count`.
    #[tracing::instrument(skip(self, state), fields(nous_id = %state.nous_id))]
    pub async fn run_audit(&self, state: &ProsocheState) -> ProsocheAuditOutcome {
        let mut all_findings: Vec<Finding> = Vec::new();
        let mut check_summary: Vec<CheckSummary> = Vec::new();
        let mut check_provenances: Vec<CheckProvenance> = Vec::new();
        let mut check_failures: Vec<CheckFailure> = Vec::new();

        for check in &self.checks {
            let kind = check.kind();
            check_provenances.push(check.metadata(state));

            let state_clone = state.clone();
            let check_arc = Arc::clone(check);
            let span = tracing::Span::current();

            match tokio::spawn(async move { check_arc.check(&state_clone).await }.instrument(span))
                .await
            {
                Ok(findings) => {
                    let count = findings.len();
                    all_findings.extend(findings);
                    check_summary.push(CheckSummary {
                        kind,
                        findings_count: count,
                    });
                }
                Err(join_err) => {
                    let reason = format!("check task failed: {join_err}");
                    tracing::warn!(
                        check_kind = %kind,
                        error = %join_err,
                        "prosoche check failed"
                    );
                    check_failures.push(CheckFailure { kind, reason });
                    check_summary.push(CheckSummary {
                        kind,
                        findings_count: 0,
                    });
                }
            }
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

        let provenance = ProsocheReportProvenance {
            report_version: "1.1.0".to_owned(),
            daemon_version: format!("oikonomos@{}", env!("CARGO_PKG_VERSION")),
            code_sha: option_env!("ALETHEIA_CODE_SHA").map(String::from),
            build_sha: option_env!("ALETHEIA_BUILD_SHA").map(String::from),
            config_hash: option_env!("ALETHEIA_CONFIG_HASH").map(String::from),
            source_query_hash: state.source_query_hash(),
            source_snapshot_hash: state.source_snapshot_hash(),
            checks: check_provenances,
            check_failures,
        };

        let report = AuditReport {
            audited_at: state.checked_at.clone(),
            nous_id: state.nous_id.clone(),
            findings: all_findings,
            check_summary,
            meta,
            provenance: Some(provenance),
        };

        // WHY: a persist failure is logged but never fails the audit — the
        // report is still returned to the caller inside the outcome.
        let (persisted_path, last_persist_error) = match self.storage.persist(&report).await {
            Ok(path) => {
                tracing::info!(
                    nous_id = %state.nous_id,
                    findings_count = total,
                    path = %path.display(),
                    "prosoche self-audit complete"
                );
                (Some(path), None)
            }
            Err(e) => {
                tracing::warn!(
                    nous_id = %state.nous_id,
                    findings_count = total,
                    error = %e,
                    "prosoche self-audit complete — report persist failed"
                );
                (None, Some(e.to_string()))
            }
        };

        ProsocheAuditOutcome {
            report,
            persisted_path,
            last_persist_error,
        }
    }
}

#[cfg(test)]
#[path = "prosoche_audit_tests.rs"]
mod tests;
