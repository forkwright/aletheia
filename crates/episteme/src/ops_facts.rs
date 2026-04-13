//! Operational fact extraction: converts runtime metrics into knowledge graph facts.
//!
//! Pipes session counts, tool success rates, error counts, and task execution
//! latencies into the knowledge graph so agents can reason about system health
//! during bootstrap ("system health is good", "tool X has been failing").

use serde::{Deserialize, Serialize};
use snafu::ResultExt;

use crate::knowledge::{
    EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactTemporal, FactType,
    MemoryScope, far_future,
};

/// Snapshot of operational metrics at a point in time.
///
/// Populated by the caller from whatever metric sources are available
/// (Prometheus counters, daemon state, session stores). The extractor
/// converts this into knowledge graph facts.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpsSnapshot {
    /// Which nous these metrics belong to.
    pub nous_id: String,
    /// Total active sessions at snapshot time.
    pub active_session_count: u64,
    /// Total tool calls observed in the current window.
    pub tool_call_total: u64,
    /// Successful tool calls in the current window.
    pub tool_call_successes: u64,
    /// Total errors observed in the current window.
    pub error_count: u64,
    /// Average task execution latency in milliseconds (0 if no tasks ran).
    pub avg_task_latency_ms: u64,
    /// Number of tasks that contributed to the latency average.
    pub task_sample_count: u64,
}

/// A fact extracted from an operational snapshot, ready for insertion.
#[derive(Debug, Clone)]
pub struct OpsFact {
    /// The knowledge graph fact.
    pub fact: Fact,
}

/// Extracts knowledge graph facts from operational metric snapshots.
///
/// Each extraction produces up to 4 facts:
/// - `ops.sessions`: active session count
/// - `ops.tool_success_rate`: tool call success rate percentage
/// - `ops.error_count`: error count in the observation window
/// - `ops.task_latency`: average task execution latency
pub struct OpsFactExtractor;

/// Default minimum tool calls before success rate is meaningful.
///
/// Callers should prefer the value from `taxis::config::KnowledgeConfig::instinct_min_tool_calls`.
pub const DEFAULT_MIN_TOOL_CALLS: u64 = 5;

impl OpsFactExtractor {
    /// Extract operational facts from a metric snapshot.
    ///
    /// `min_tool_calls` is the minimum tool calls before success rate is meaningful.
    /// Sourced from `taxis::config::KnowledgeConfig::instinct_min_tool_calls`.
    ///
    /// Returns a `Vec` of facts suitable for insertion into the knowledge store.
    /// Facts with insufficient data (e.g., zero tool calls) are omitted.
    ///
    /// # Errors
    ///
    /// Returns an error if fact ID generation fails (should not happen in practice).
    pub fn extract(snapshot: &OpsSnapshot, min_tool_calls: u64) -> Result<Vec<OpsFact>, ExtractError> {
        let now = jiff::Timestamp::now();
        let mut facts = Vec::with_capacity(4);

        // 1. Active session count
        facts.push(build_ops_fact(
            &snapshot.nous_id,
            "ops.sessions",
            &format!("active sessions: {}", snapshot.active_session_count),
            confidence_from_count(snapshot.active_session_count),
            now,
        )?);

        // 2. Tool success rate (only if enough data)
        if snapshot.tool_call_total >= min_tool_calls {
            // WHY: u64 counts are divided to produce a [0.0, 1.0] ratio.
            // Saturating to u32::MAX before converting to f64 avoids precision loss
            // on absurdly large counts (never expected in practice).
            let successes =
                f64::from(u32::try_from(snapshot.tool_call_successes).unwrap_or(u32::MAX));
            let total = f64::from(u32::try_from(snapshot.tool_call_total).unwrap_or(u32::MAX));
            let rate = successes / total * 100.0;
            let confidence = rate / 100.0;
            facts.push(build_ops_fact(
                &snapshot.nous_id,
                "ops.tool_success_rate",
                &format!(
                    "tool success rate: {rate:.1}% ({}/{} calls)",
                    snapshot.tool_call_successes, snapshot.tool_call_total,
                ),
                confidence,
                now,
            )?);
        }

        // 3. Error count
        facts.push(build_ops_fact(
            &snapshot.nous_id,
            "ops.error_count",
            &format!("error count: {}", snapshot.error_count),
            error_confidence(snapshot.error_count),
            now,
        )?);

        // 4. Task latency (only if tasks ran)
        if snapshot.task_sample_count > 0 {
            facts.push(build_ops_fact(
                &snapshot.nous_id,
                "ops.task_latency",
                &format!(
                    "avg task latency: {}ms (over {} tasks)",
                    snapshot.avg_task_latency_ms, snapshot.task_sample_count,
                ),
                latency_confidence(snapshot.avg_task_latency_ms),
                now,
            )?);
        }

        Ok(facts)
    }
}

/// Build a single operational fact with standard metadata.
fn build_ops_fact(
    nous_id: &str,
    fact_type_tag: &str,
    content: &str,
    confidence: f64,
    now: jiff::Timestamp,
) -> Result<OpsFact, ExtractError> {
    let fact_id = crate::id::FactId::new(format!("{fact_type_tag}-{}", koina::ulid::Ulid::new()))
        .context(FactIdSnafu)?;

    Ok(OpsFact {
        fact: Fact {
            id: fact_id,
            nous_id: nous_id.to_owned(),
            fact_type: String::from("operational"),
            content: content.to_owned(),
            scope: Some(MemoryScope::Project),
            temporal: FactTemporal {
                valid_from: now,
                valid_to: far_future(),
                recorded_at: now,
            },
            provenance: FactProvenance {
                confidence,
                tier: EpistemicTier::Inferred,
                source_session_id: None,
                stability_hours: FactType::Operational.base_stability_hours(),
            },
            lifecycle: FactLifecycle {
                superseded_by: None,
                is_forgotten: false,
                forgotten_at: None,
                forget_reason: None,
            },
            access: FactAccess {
                access_count: 0,
                last_accessed_at: None,
            },
        },
    })
}

/// Confidence for session count: 1.0 (exact metric).
fn confidence_from_count(_count: u64) -> f64 {
    // WHY: session count is an exact value from the store, not an estimate.
    1.0
}

/// Confidence inversely related to error count: more errors = lower confidence
/// in system health.
fn error_confidence(error_count: u64) -> f64 {
    match error_count {
        0 => 1.0,
        1..=5 => 0.8,
        6..=20 => 0.5,
        _ => 0.3,
    }
}

/// Confidence based on task latency: low latency = high confidence in health.
fn latency_confidence(avg_ms: u64) -> f64 {
    match avg_ms {
        0..=100 => 1.0,
        101..=500 => 0.9,
        501..=2000 => 0.7,
        2001..=10_000 => 0.5,
        _ => 0.3,
    }
}

/// Errors from operational fact extraction.
#[derive(Debug, snafu::Snafu)]
#[non_exhaustive]
pub enum ExtractError {
    /// Failed to create a fact ID.
    #[snafu(display("failed to create operational fact ID: {source}"))]
    FactId {
        /// The underlying ID validation error.
        source: crate::id::IdValidationError,
    },
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn extract_produces_facts_for_full_snapshot() {
        let snapshot = OpsSnapshot {
            nous_id: String::from("test-nous"),
            active_session_count: 3,
            tool_call_total: 20,
            tool_call_successes: 18,
            error_count: 2,
            avg_task_latency_ms: 150,
            task_sample_count: 5,
        };

        let facts = OpsFactExtractor::extract(&snapshot, DEFAULT_MIN_TOOL_CALLS).expect("extraction should succeed");
        assert_eq!(facts.len(), 4, "full snapshot should produce 4 facts");

        for ops_fact in &facts {
            assert_eq!(ops_fact.fact.fact_type, "operational");
            assert_eq!(ops_fact.fact.nous_id, "test-nous");
            assert_eq!(ops_fact.fact.scope, Some(MemoryScope::Project));
            assert!(
                ops_fact.fact.provenance.confidence > 0.0
                    && ops_fact.fact.provenance.confidence <= 1.0,
                "confidence should be in (0, 1]"
            );
        }
    }

    #[test]
    fn extract_skips_tool_rate_with_insufficient_data() {
        let snapshot = OpsSnapshot {
            nous_id: String::from("test-nous"),
            active_session_count: 1,
            tool_call_total: 2,
            tool_call_successes: 2,
            error_count: 0,
            avg_task_latency_ms: 0,
            task_sample_count: 0,
        };

        let facts = OpsFactExtractor::extract(&snapshot, DEFAULT_MIN_TOOL_CALLS).expect("extraction should succeed");
        // sessions + errors = 2 (no tool rate, no latency)
        assert_eq!(
            facts.len(),
            2,
            "should skip tool rate and latency when insufficient data"
        );
    }

    #[test]
    fn extract_empty_snapshot_produces_baseline_facts() {
        let snapshot = OpsSnapshot {
            nous_id: String::from("test-nous"),
            ..Default::default()
        };

        let facts = OpsFactExtractor::extract(&snapshot, DEFAULT_MIN_TOOL_CALLS).expect("extraction should succeed");
        // sessions + errors = 2
        assert_eq!(
            facts.len(),
            2,
            "empty snapshot should produce 2 baseline facts"
        );
    }

    #[test]
    fn fact_content_is_human_readable() {
        let snapshot = OpsSnapshot {
            nous_id: String::from("test-nous"),
            active_session_count: 5,
            tool_call_total: 100,
            tool_call_successes: 95,
            error_count: 3,
            avg_task_latency_ms: 250,
            task_sample_count: 10,
        };

        let facts = OpsFactExtractor::extract(&snapshot, DEFAULT_MIN_TOOL_CALLS).expect("extraction should succeed");
        let contents: Vec<&str> = facts.iter().map(|f| f.fact.content.as_str()).collect();

        assert!(
            contents.iter().any(|c| c.contains("active sessions: 5")),
            "should contain session count"
        );
        assert!(
            contents.iter().any(|c| c.contains("tool success rate")),
            "should contain tool success rate"
        );
        assert!(
            contents.iter().any(|c| c.contains("error count: 3")),
            "should contain error count"
        );
        assert!(
            contents
                .iter()
                .any(|c| c.contains("avg task latency: 250ms")),
            "should contain task latency"
        );
    }

    #[test]
    fn error_confidence_decreases_with_more_errors() {
        assert!(error_confidence(0) > error_confidence(5));
        assert!(error_confidence(5) > error_confidence(20));
        assert!(error_confidence(20) > error_confidence(100));
    }

    #[test]
    fn latency_confidence_decreases_with_higher_latency() {
        assert!(latency_confidence(50) > latency_confidence(300));
        assert!(latency_confidence(300) > latency_confidence(1000));
        assert!(latency_confidence(1000) > latency_confidence(5000));
    }

    #[test]
    fn operational_fact_type_stability() {
        let stability = FactType::Operational.base_stability_hours();
        assert!(
            (stability - 72.0).abs() < f64::EPSILON,
            "operational facts should have 3-day (72h) stability"
        );
    }
}
