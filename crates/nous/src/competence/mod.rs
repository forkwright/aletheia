//! Per-agent per-domain competence tracking with rolling statistics.
//!
//! Persisted in a fjall keyspace. One row per (agent, domain) tracks the
//! rolling score; a separate outcome log enables windowed statistics.
//!
//! # Key schema
//!
//! All keys are UTF-8 strings. Values are JSON-encoded domain structs.
//!
//! | Partition   | Key pattern                                         | Value                  |
//! |-------------|-----------------------------------------------------|------------------------|
//! | `domains`   | `{nous_id}:{domain}`                                | JSON `DomainScore`     |
//! | `outcomes`  | `{nous_id}:{domain}:{recorded_at}:{seq_padded_20}`  | JSON `OutcomeRecord`   |
//! | `counters`  | `{nous_id}:{domain}`                                | big-endian `u64`       |
//!
//! Outcome keys embed `recorded_at` (ISO 8601, lexicographic-sortable) plus a
//! zero-padded sequence so insertion order is stable under concurrent writes
//! at the same timestamp.

use std::path::Path;
use std::sync::{Arc, Mutex};

use fjall::{KeyspaceCreateOptions, SingleWriterTxDatabase};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::error;

/// Width for zero-padded sequence numbers.
const SEQ_WIDTH: usize = 20;

/// Partitions used by the competence tracker.
const PARTITIONS: &[&str] = &["domains", "outcomes", "counters"];

/// Format a u64 as a zero-padded key component so lexicographic ordering
/// matches numeric ordering.
fn pad_u64(v: u64) -> String {
    format!("{v:0>SEQ_WIDTH$}")
}

/// Decode a big-endian u64 from 8 bytes.
fn decode_u64(bytes: &[u8]) -> u64 {
    let arr: [u8; 8] = bytes
        .get(..8)
        .and_then(|s| s.try_into().ok())
        .unwrap_or([0u8; 8]);
    u64::from_be_bytes(arr)
}

/// Encode a u64 as big-endian bytes.
fn encode_u64(v: u64) -> [u8; 8] {
    v.to_be_bytes()
}

/// Per-agent competence scoring configuration.
///
/// All defaults match the constants they replace so behaviour is identical
/// when the tracker is constructed with `CompetenceConfig::default()`.
#[derive(Debug, Clone)]
pub struct CompetenceConfig {
    /// Competence score penalty per correction. Default: 0.05.
    pub correction_penalty: f64,
    /// Competence score bonus per successful turn. Default: 0.02.
    pub success_bonus: f64,
    /// Competence score penalty per user disagreement. Default: 0.01.
    pub disagreement_penalty: f64,
    /// Competence score floor. Default: 0.1.
    pub min_score: f64,
    /// Competence score ceiling. Default: 0.95.
    pub max_score: f64,
    /// Initial competence score for a new agent. Default: 0.5.
    pub default_score: f64,
    /// Competence score below which escalation fires. Default: 0.30.
    pub escalation_failure_threshold: f64,
    /// Minimum samples before escalation threshold is evaluated. Default: 5.
    pub escalation_min_samples: u32,
}

impl Default for CompetenceConfig {
    fn default() -> Self {
        let b = taxis::config::AgentBehaviorDefaults::default();
        Self {
            correction_penalty: b.competence_correction_penalty,
            success_bonus: b.competence_success_bonus,
            disagreement_penalty: b.competence_disagreement_penalty,
            min_score: b.competence_min_score,
            max_score: b.competence_max_score,
            default_score: b.competence_default_score,
            escalation_failure_threshold: b.competence_escalation_failure_threshold,
            escalation_min_samples: b.competence_escalation_min_samples,
        }
    }
}

impl CompetenceConfig {
    /// Build from a resolved agent behavior config.
    #[must_use]
    pub fn from_behavior(behavior: &taxis::config::AgentBehaviorDefaults) -> Self {
        Self {
            correction_penalty: behavior.competence_correction_penalty,
            success_bonus: behavior.competence_success_bonus,
            disagreement_penalty: behavior.competence_disagreement_penalty,
            min_score: behavior.competence_min_score,
            max_score: behavior.competence_max_score,
            default_score: behavior.competence_default_score,
            escalation_failure_threshold: behavior.competence_escalation_failure_threshold,
            escalation_min_samples: behavior.competence_escalation_min_samples,
        }
    }
}

/// Task outcome for competence tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum TaskOutcome {
    /// Task completed successfully.
    Success,
    /// Task partially completed.
    Partial,
    /// Task failed.
    Failure,
}

impl TaskOutcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Partial => "partial",
            Self::Failure => "failure",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "success" => Some(Self::Success),
            "partial" => Some(Self::Partial),
            "failure" => Some(Self::Failure),
            _ => None,
        }
    }
}

/// Per-domain competence score for an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainScore {
    /// Domain name (e.g., "coding", "research").
    pub domain: String,
    /// Competence score (0.0--1.0), starts at 0.5.
    pub score: f64,
    /// Total successes recorded.
    pub successes: u32,
    /// Total partial completions recorded.
    pub partials: u32,
    /// Total failures recorded.
    pub failures: u32,
    /// Operator corrections (decreases score).
    pub corrections: u32,
    /// Cross-agent disagreements (decreases score).
    pub disagreements: u32,
    /// Last update timestamp.
    pub updated_at: String,
}

/// Agent-level competence summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCompetence {
    /// Agent identifier.
    // kanon:ignore RUST/primitive-for-domain-id — existing String-based ID; migrating to newtype requires cross-crate API changes
    pub nous_id: String,
    /// Per-domain scores.
    pub domains: Vec<DomainScore>,
    /// Weighted average of domain scores.
    pub overall_score: f64,
}

/// Model escalation recommendation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscalationRecommendation {
    /// Domain triggering the recommendation.
    pub domain: String,
    /// Current failure rate.
    pub failure_rate: f64,
    /// Current agent score in this domain.
    pub current_score: f64,
    /// Whether escalation to a higher-tier model is recommended.
    pub should_escalate: bool,
}

/// An outcome entry: the per-outcome log row used for rolling statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct OutcomeRecord {
    outcome: String,
    recorded_at: String,
}

/// Tracks agent competence per domain with fjall persistence.
pub struct CompetenceTracker {
    db: Arc<SingleWriterTxDatabase>,
    /// Shared write mutex — serializes writers.
    write_lock: Mutex<()>,
    /// Kept alive to auto-delete the temp directory when the store is dropped.
    _temp_dir: Option<tempfile::TempDir>,
    config: CompetenceConfig,
}

impl CompetenceTracker {
    /// Open a file-backed competence tracker.
    ///
    /// # Errors
    ///
    /// Returns `CompetenceStore` if the database cannot be opened or initialized.
    pub fn open(path: &Path, config: CompetenceConfig) -> error::Result<Self> {
        let fdb = koina::fjall::FjallDb::open(path, PARTITIONS).map_err(|e| {
            error::CompetenceStoreSnafu {
                message: format!("failed to open competence database: {e}"),
            }
            .build()
        })?;
        Ok(Self::from_fjall_db(fdb, config))
    }

    /// Open an in-memory competence tracker (for testing).
    ///
    /// The directory and all data are deleted when the returned tracker is
    /// dropped.
    ///
    /// # Errors
    ///
    /// Returns `CompetenceStore` if the schema cannot be created.
    pub fn open_in_memory() -> error::Result<Self> {
        let fdb = koina::fjall::FjallDb::open_temp(PARTITIONS).map_err(|e| {
            error::CompetenceStoreSnafu {
                message: format!("failed to open in-memory competence database: {e}"),
            }
            .build()
        })?;
        Ok(Self::from_fjall_db(fdb, CompetenceConfig::default()))
    }

    fn from_fjall_db(fdb: koina::fjall::FjallDb, config: CompetenceConfig) -> Self {
        Self {
            db: Arc::new(fdb.db),
            write_lock: fdb.write_lock,
            _temp_dir: fdb._temp_dir,
            config,
        }
    }

    fn partition(&self, name: &str) -> error::Result<fjall::SingleWriterTxKeyspace> {
        self.db
            .keyspace(name, KeyspaceCreateOptions::default)
            .map_err(|e| {
                error::CompetenceStoreSnafu {
                    message: format!("fjall partition {name}: {e}"),
                }
                .build()
            })
    }

    fn domain_key(nous_id: &str, domain: &str) -> String {
        format!("{nous_id}:{domain}")
    }

    fn outcome_key(nous_id: &str, domain: &str, recorded_at: &str, seq: u64) -> String {
        format!("{nous_id}:{domain}:{recorded_at}:{}", pad_u64(seq))
    }

    fn counter_key(nous_id: &str, domain: &str) -> String {
        format!("{nous_id}:{domain}")
    }

    fn read_domain(
        &self,
        domains: &fjall::SingleWriterTxKeyspace,
        nous_id: &str,
        domain: &str,
    ) -> error::Result<Option<DomainScore>> {
        use fjall::Readable;
        let snap = self.db.read_tx();
        let key = Self::domain_key(nous_id, domain);
        let Some(bytes) = snap.get(domains, key.as_bytes()).map_err(|e| {
            error::CompetenceStoreSnafu {
                message: format!("fjall domain read: {e}"),
            }
            .build()
        })?
        else {
            return Ok(None);
        };
        let ds: DomainScore = serde_json::from_slice(&bytes).map_err(|e| {
            error::CompetenceStoreSnafu {
                message: format!("domain json decode: {e}"),
            }
            .build()
        })?;
        Ok(Some(ds))
    }

    fn ensure_domain_record(
        &self,
        tx: &mut fjall::SingleWriterWriteTx<'_>,
        domains: &fjall::SingleWriterTxKeyspace,
        nous_id: &str,
        domain: &str,
        now: &str,
    ) -> error::Result<DomainScore> {
        use fjall::Readable;
        let key = Self::domain_key(nous_id, domain);
        if let Some(bytes) = tx.get(domains, key.as_bytes()).unwrap_or(None) {
            let ds: DomainScore = serde_json::from_slice(&bytes).map_err(|e| {
                error::CompetenceStoreSnafu {
                    message: format!("domain json decode: {e}"),
                }
                .build()
            })?;
            return Ok(ds);
        }
        let ds = DomainScore {
            domain: domain.to_owned(),
            score: self.config.default_score,
            successes: 0,
            partials: 0,
            failures: 0,
            corrections: 0,
            disagreements: 0,
            updated_at: now.to_owned(),
        };
        let data = serde_json::to_vec(&ds).map_err(|e| {
            error::CompetenceStoreSnafu {
                message: format!("domain json encode: {e}"),
            }
            .build()
        })?;
        tx.insert(domains, key.as_str(), data.as_slice());
        Ok(ds)
    }

    fn write_domain(
        tx: &mut fjall::SingleWriterWriteTx<'_>,
        domains: &fjall::SingleWriterTxKeyspace,
        nous_id: &str,
        ds: &DomainScore,
    ) -> error::Result<()> {
        let key = Self::domain_key(nous_id, &ds.domain);
        let data = serde_json::to_vec(ds).map_err(|e| {
            error::CompetenceStoreSnafu {
                message: format!("domain json encode: {e}"),
            }
            .build()
        })?;
        tx.insert(domains, key.as_str(), data.as_slice());
        Ok(())
    }

    fn next_outcome_seq(
        tx: &mut fjall::SingleWriterWriteTx<'_>,
        counters: &fjall::SingleWriterTxKeyspace,
        nous_id: &str,
        domain: &str,
    ) -> u64 {
        use fjall::Readable;
        let key = Self::counter_key(nous_id, domain);
        let current = tx
            .get(counters, key.as_bytes())
            .unwrap_or(None)
            .map_or(0, |b| decode_u64(&b));
        current + 1
    }

    /// Record a task outcome for an agent in a domain.
    ///
    /// # Errors
    ///
    /// Returns `CompetenceStore` on database write failure.
    pub fn record_outcome(
        &self,
        nous_id: &str,
        domain: &str,
        outcome: TaskOutcome,
    ) -> error::Result<()> {
        let now = Timestamp::now().to_string();

        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let domains = self.partition("domains")?;
        let outcomes = self.partition("outcomes")?;
        let counters = self.partition("counters")?;

        let mut tx = self.db.write_tx();

        // Ensure the domain row exists (initialized with default score).
        let mut ds = self.ensure_domain_record(&mut tx, &domains, nous_id, domain, &now)?;

        // Apply score delta.
        let score_delta = match outcome {
            TaskOutcome::Success => self.config.success_bonus,
            TaskOutcome::Partial => 0.0,
            TaskOutcome::Failure => -self.config.correction_penalty,
        };
        ds.score = (ds.score + score_delta)
            .max(self.config.min_score)
            .min(self.config.max_score);
        match outcome {
            TaskOutcome::Success => ds.successes += 1,
            TaskOutcome::Partial => ds.partials += 1,
            TaskOutcome::Failure => ds.failures += 1,
        }
        ds.updated_at.clone_from(&now);

        tracing::debug!(
            score_delta,
            min_score = self.config.min_score,
            max_score = self.config.max_score,
            ?outcome,
            "competence record_outcome"
        );

        Self::write_domain(&mut tx, &domains, nous_id, &ds)?;

        // Append outcome log entry.
        let seq = Self::next_outcome_seq(&mut tx, &counters, nous_id, domain);
        let rec = OutcomeRecord {
            outcome: outcome.as_str().to_owned(),
            recorded_at: now.clone(),
        };
        let rec_data = serde_json::to_vec(&rec).map_err(|e| {
            error::CompetenceStoreSnafu {
                message: format!("outcome json encode: {e}"),
            }
            .build()
        })?;
        let outcome_key = Self::outcome_key(nous_id, domain, &now, seq);
        tx.insert(&outcomes, outcome_key.as_str(), rec_data.as_slice());
        let counter_key = Self::counter_key(nous_id, domain);
        tx.insert(&counters, counter_key.as_str(), encode_u64(seq));

        tx.commit().map_err(|e| {
            error::CompetenceStoreSnafu {
                message: format!("fjall commit record_outcome: {e}"),
            }
            .build()
        })?;

        Ok(())
    }

    /// Record an operator correction for an agent in a domain.
    ///
    /// # Errors
    ///
    /// Returns `CompetenceStore` on database write failure.
    pub fn record_correction(&self, nous_id: &str, domain: &str) -> error::Result<()> {
        let now = Timestamp::now().to_string();

        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let domains = self.partition("domains")?;

        let mut tx = self.db.write_tx();
        let mut ds = self.ensure_domain_record(&mut tx, &domains, nous_id, domain, &now)?;
        ds.score = (ds.score - self.config.correction_penalty).max(self.config.min_score);
        ds.corrections += 1;
        ds.updated_at.clone_from(&now);

        tracing::debug!(
            correction_penalty = self.config.correction_penalty,
            min_score = self.config.min_score,
            nous_id,
            domain,
            "competence record_correction"
        );

        Self::write_domain(&mut tx, &domains, nous_id, &ds)?;
        tx.commit().map_err(|e| {
            error::CompetenceStoreSnafu {
                message: format!("fjall commit record_correction: {e}"),
            }
            .build()
        })?;
        Ok(())
    }

    /// Record a cross-agent disagreement for an agent in a domain.
    ///
    /// # Errors
    ///
    /// Returns `CompetenceStore` on database write failure.
    pub fn record_disagreement(&self, nous_id: &str, domain: &str) -> error::Result<()> {
        let now = Timestamp::now().to_string();

        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let domains = self.partition("domains")?;

        let mut tx = self.db.write_tx();
        let mut ds = self.ensure_domain_record(&mut tx, &domains, nous_id, domain, &now)?;
        ds.score = (ds.score - self.config.disagreement_penalty).max(self.config.min_score);
        ds.disagreements += 1;
        ds.updated_at.clone_from(&now);

        tracing::debug!(
            disagreement_penalty = self.config.disagreement_penalty,
            min_score = self.config.min_score,
            nous_id,
            domain,
            "competence record_disagreement"
        );

        Self::write_domain(&mut tx, &domains, nous_id, &ds)?;
        tx.commit().map_err(|e| {
            error::CompetenceStoreSnafu {
                message: format!("fjall commit record_disagreement: {e}"),
            }
            .build()
        })?;
        Ok(())
    }

    /// Get the competence score for an agent in a domain.
    ///
    /// Returns the default score (0.5) if no data exists.
    ///
    /// # Errors
    ///
    /// Returns `CompetenceStore` on database read failure.
    pub fn score(&self, nous_id: &str, domain: &str) -> error::Result<f64> {
        let domains = self.partition("domains")?;
        Ok(self
            .read_domain(&domains, nous_id, domain)?
            .map_or(self.config.default_score, |ds| ds.score))
    }

    /// Get full competence data for an agent across all domains.
    ///
    /// # Errors
    ///
    /// Returns `CompetenceStore` on database read failure.
    pub fn agent_competence(&self, nous_id: &str) -> error::Result<AgentCompetence> {
        use fjall::Readable;

        let domains_part = self.partition("domains")?;
        let snap = self.db.read_tx();
        let prefix = format!("{nous_id}:");
        let upper = format!("{nous_id};\x00");

        let mut domains: Vec<DomainScore> = Vec::new();
        for guard in snap.range(&domains_part, prefix.as_str()..upper.as_str()) {
            if let Ok((_k, v)) = guard.into_inner()
                && let Ok(ds) = serde_json::from_slice::<DomainScore>(&v)
            {
                domains.push(ds);
            }
        }

        // Sort by domain name to match SQL ORDER BY domain.
        domains.sort_by(|a, b| a.domain.cmp(&b.domain));

        let overall_score = if domains.is_empty() {
            self.config.default_score
        } else {
            // WHY f64::from(u32): competence domain count is under 100
            // (one per skill area), so `try_from` is infallible in
            // practice; u32→f64 is an exact conversion.
            let len_u32 = u32::try_from(domains.len()).unwrap_or(u32::MAX);
            let len = f64::from(len_u32);
            domains.iter().map(|d| d.score).sum::<f64>() / len
        };

        Ok(AgentCompetence {
            nous_id: nous_id.to_owned(),
            domains,
            overall_score,
        })
    }

    /// Get rolling statistics for an agent in a domain within a recent window.
    ///
    /// Returns (successes, partials, failures) within the last `window_size` outcomes.
    ///
    /// # Errors
    ///
    /// Returns `CompetenceStore` on database read failure.
    pub fn rolling_stats(
        &self,
        nous_id: &str,
        domain: &str,
        window_size: u32,
    ) -> error::Result<RollingStats> {
        use fjall::Readable;

        let outcomes_part = self.partition("outcomes")?;
        let snap = self.db.read_tx();
        let prefix = format!("{nous_id}:{domain}:");
        let upper = format!("{nous_id}:{domain};\x00");

        // Collect all outcomes for this (nous, domain) in key order (ascending by
        // recorded_at then seq).
        let mut outcome_strs: Vec<String> = Vec::new();
        for guard in snap.range(&outcomes_part, prefix.as_str()..upper.as_str()) {
            if let Ok((_k, v)) = guard.into_inner()
                && let Ok(rec) = serde_json::from_slice::<OutcomeRecord>(&v)
            {
                outcome_strs.push(rec.outcome);
            }
        }

        // Window = last N outcomes (most recent).
        let limit = usize::try_from(window_size).unwrap_or(usize::MAX);
        let windowed: Vec<&str> = if outcome_strs.len() > limit {
            let start = outcome_strs.len() - limit;
            outcome_strs
                .iter()
                .skip(start)
                .map(String::as_str)
                .collect()
        } else {
            outcome_strs.iter().map(String::as_str).collect()
        };

        let mut stats = RollingStats {
            window_size,
            total: u32::try_from(windowed.len()).unwrap_or(u32::MAX),
            successes: 0,
            partials: 0,
            failures: 0,
        };

        for outcome_str in &windowed {
            match TaskOutcome::from_str(outcome_str) {
                Some(TaskOutcome::Success) => stats.successes += 1,
                Some(TaskOutcome::Partial) => stats.partials += 1,
                Some(TaskOutcome::Failure) => stats.failures += 1,
                None => {}
            }
        }

        Ok(stats)
    }

    /// Check whether an agent should escalate to a higher-tier model for a domain.
    ///
    /// Escalation is recommended when the failure rate exceeds the configured
    /// threshold with at least the minimum number of recorded outcomes.
    ///
    /// # Errors
    ///
    /// Returns `CompetenceStore` on database read failure.
    pub fn escalation_recommendation(
        &self,
        nous_id: &str,
        domain: &str,
    ) -> error::Result<EscalationRecommendation> {
        let stats = self.rolling_stats(nous_id, domain, 20)?;
        let current_score = self.score(nous_id, domain)?;

        let escalation_min_samples = self.config.escalation_min_samples;
        let escalation_failure_threshold = self.config.escalation_failure_threshold;

        tracing::debug!(
            escalation_min_samples,
            escalation_failure_threshold,
            total = stats.total,
            failures = stats.failures,
            "competence escalation_recommendation"
        );

        let failure_rate = if stats.total >= escalation_min_samples {
            f64::from(stats.failures) / f64::from(stats.total)
        } else {
            0.0
        };

        let should_escalate =
            stats.total >= escalation_min_samples && failure_rate > escalation_failure_threshold;

        Ok(EscalationRecommendation {
            domain: domain.to_owned(),
            failure_rate,
            current_score,
            should_escalate,
        })
    }
}

/// Rolling outcome statistics within a configurable window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollingStats {
    /// Configured window size.
    pub window_size: u32,
    /// Actual number of outcomes in the window.
    pub total: u32,
    /// Successes within the window.
    pub successes: u32,
    /// Partial completions within the window.
    pub partials: u32,
    /// Failures within the window.
    pub failures: u32,
}

impl RollingStats {
    /// Failure rate within the window (0.0 if no outcomes).
    #[must_use]
    pub fn failure_rate(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        f64::from(self.failures) / f64::from(self.total)
    }

    /// Success rate within the window (0.0 if no outcomes).
    #[must_use]
    pub fn success_rate(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        f64::from(self.successes) / f64::from(self.total)
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests;
