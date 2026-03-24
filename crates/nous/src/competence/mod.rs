//! Per-agent per-domain competence tracking with rolling statistics.

use jiff::Timestamp;
#[expect(
    clippy::disallowed_types,
    reason = "competence tracker owns its own isolated SQLite file; not part of the shared SessionStore pipeline"
)]
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use snafu::ResultExt as _;

use crate::error;

const CORRECTION_PENALTY: f64 = 0.05;
const SUCCESS_BONUS: f64 = 0.02;
const DISAGREEMENT_PENALTY: f64 = 0.01;
const MIN_SCORE: f64 = 0.1;
const MAX_SCORE: f64 = 0.95;
const DEFAULT_SCORE: f64 = 0.5;

/// Failure rate threshold above which model escalation is recommended.
const ESCALATION_FAILURE_THRESHOLD: f64 = 0.30;

/// Minimum number of recorded outcomes before escalation logic activates.
const ESCALATION_MIN_SAMPLES: u32 = 5;

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

/// Tracks agent competence per domain with `SQLite` persistence.
#[expect(
    clippy::disallowed_types,
    reason = "competence tracker owns its own isolated SQLite file; not part of the shared SessionStore pipeline"
)]
pub struct CompetenceTracker {
    conn: Connection,
}

impl CompetenceTracker {
    /// Open a file-backed competence tracker.
    ///
    /// # Errors
    ///
    /// Returns `CompetenceStore` if the database cannot be opened or initialized.
    #[expect(
        clippy::disallowed_types,
        reason = "competence tracker owns its own isolated SQLite file; not part of the shared SessionStore pipeline"
    )]
    pub fn open(path: &std::path::Path) -> error::Result<Self> {
        let conn = Connection::open(path).context(error::CompetenceStoreSnafu {
            message: "failed to open competence database",
        })?;
        Self::init(conn)
    }

    /// Open an in-memory competence tracker (for testing).
    ///
    /// # Errors
    ///
    /// Returns `CompetenceStore` if the schema cannot be created.
    #[expect(
        clippy::disallowed_types,
        reason = "competence tracker owns its own isolated SQLite file; not part of the shared SessionStore pipeline"
    )]
    pub fn open_in_memory() -> error::Result<Self> {
        let conn = Connection::open_in_memory().context(error::CompetenceStoreSnafu {
            message: "failed to open in-memory competence database",
        })?;
        Self::init(conn)
    }

    #[expect(
        clippy::disallowed_types,
        reason = "competence tracker owns its own isolated SQLite file; not part of the shared SessionStore pipeline"
    )]
    fn init(conn: Connection) -> error::Result<Self> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;

             CREATE TABLE IF NOT EXISTS competence_domains (
                 nous_id    TEXT NOT NULL,
                 domain     TEXT NOT NULL,
                 score      REAL NOT NULL DEFAULT 0.5,
                 successes  INTEGER NOT NULL DEFAULT 0,
                 partials   INTEGER NOT NULL DEFAULT 0,
                 failures   INTEGER NOT NULL DEFAULT 0,
                 corrections   INTEGER NOT NULL DEFAULT 0,
                 disagreements INTEGER NOT NULL DEFAULT 0,
                 updated_at TEXT NOT NULL,
                 PRIMARY KEY (nous_id, domain)
             );

             CREATE TABLE IF NOT EXISTS competence_outcomes (
                 id         INTEGER PRIMARY KEY AUTOINCREMENT,
                 nous_id    TEXT NOT NULL,
                 domain     TEXT NOT NULL,
                 outcome    TEXT NOT NULL,
                 recorded_at TEXT NOT NULL
             );

             CREATE INDEX IF NOT EXISTS idx_outcomes_agent_domain
                 ON competence_outcomes (nous_id, domain, recorded_at);",
        )
        .context(error::CompetenceStoreSnafu {
            message: "failed to initialize competence schema",
        })?;

        Ok(Self { conn })
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

        let tx = self
            .conn
            .unchecked_transaction()
            .context(error::CompetenceStoreSnafu {
                message: "failed to begin transaction",
            })?;

        tx.execute(
            "INSERT INTO competence_outcomes (nous_id, domain, outcome, recorded_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![nous_id, domain, outcome.as_str(), now],
        )
        .context(error::CompetenceStoreSnafu {
            message: "failed to insert outcome",
        })?;

        Self::ensure_domain(&tx, nous_id, domain, &now)?;

        let score_delta = match outcome {
            TaskOutcome::Success => SUCCESS_BONUS,
            TaskOutcome::Partial => 0.0,
            TaskOutcome::Failure => -CORRECTION_PENALTY,
        };
        let counter_field = match outcome {
            TaskOutcome::Success => "successes",
            TaskOutcome::Partial => "partials",
            TaskOutcome::Failure => "failures",
        };

        tx.execute(
            &format!(
                "UPDATE competence_domains
                 SET score = MAX({MIN_SCORE}, MIN({MAX_SCORE}, score + ?1)),
                     {counter_field} = {counter_field} + 1,
                     updated_at = ?2
                 WHERE nous_id = ?3 AND domain = ?4"
            ),
            params![score_delta, now, nous_id, domain],
        )
        .context(error::CompetenceStoreSnafu {
            message: "failed to update domain score",
        })?;

        tx.commit().context(error::CompetenceStoreSnafu {
            message: "failed to commit outcome",
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
        Self::ensure_domain(&self.conn, nous_id, domain, &now)?;

        self.conn
            .execute(
                &format!(
                    "UPDATE competence_domains
                 SET score = MAX({MIN_SCORE}, score - ?1),
                     corrections = corrections + 1,
                     updated_at = ?2
                 WHERE nous_id = ?3 AND domain = ?4"
                ),
                params![CORRECTION_PENALTY, now, nous_id, domain],
            )
            .context(error::CompetenceStoreSnafu {
                message: "failed to record correction",
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
        Self::ensure_domain(&self.conn, nous_id, domain, &now)?;

        self.conn
            .execute(
                &format!(
                    "UPDATE competence_domains
                 SET score = MAX({MIN_SCORE}, score - ?1),
                     disagreements = disagreements + 1,
                     updated_at = ?2
                 WHERE nous_id = ?3 AND domain = ?4"
                ),
                params![DISAGREEMENT_PENALTY, now, nous_id, domain],
            )
            .context(error::CompetenceStoreSnafu {
                message: "failed to record disagreement",
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
        let result: Option<f64> = self
            .conn
            .prepare_cached(
                "SELECT score FROM competence_domains WHERE nous_id = ?1 AND domain = ?2",
            )
            .context(error::CompetenceStoreSnafu {
                message: "failed to prepare score query",
            })?
            .query_row(params![nous_id, domain], |row| row.get(0))
            .optional()
            .context(error::CompetenceStoreSnafu {
                message: "failed to query score",
            })?;

        Ok(result.unwrap_or(DEFAULT_SCORE))
    }

    /// Get full competence data for an agent across all domains.
    ///
    /// # Errors
    ///
    /// Returns `CompetenceStore` on database read failure.
    pub fn agent_competence(&self, nous_id: &str) -> error::Result<AgentCompetence> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT domain, score, successes, partials, failures,
                        corrections, disagreements, updated_at
                 FROM competence_domains
                 WHERE nous_id = ?1
                 ORDER BY domain",
            )
            .context(error::CompetenceStoreSnafu {
                message: "failed to prepare agent competence query",
            })?;

        let domains: Vec<DomainScore> = stmt
            .query_map(params![nous_id], |row| {
                Ok(DomainScore {
                    domain: row.get(0)?,
                    score: row.get(1)?,
                    successes: row.get(2)?,
                    partials: row.get(3)?,
                    failures: row.get(4)?,
                    corrections: row.get(5)?,
                    disagreements: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            })
            .context(error::CompetenceStoreSnafu {
                message: "failed to query agent competence",
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context(error::CompetenceStoreSnafu {
                message: "failed to collect domain scores",
            })?;

        let overall_score = if domains.is_empty() {
            DEFAULT_SCORE
        } else {
            #[expect(
                clippy::cast_precision_loss,
                reason = "domain count will never exceed 2^53; precision loss is not a concern here"
            )]
            #[expect(
                clippy::as_conversions,
                reason = "usize-to-f64 for averaging; domain count is bounded and safe"
            )]
            let len = domains.len() as f64;
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
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT outcome FROM competence_outcomes
                 WHERE nous_id = ?1 AND domain = ?2
                 ORDER BY recorded_at DESC
                 LIMIT ?3",
            )
            .context(error::CompetenceStoreSnafu {
                message: "failed to prepare rolling stats query",
            })?;

        let outcomes: Vec<String> = stmt
            .query_map(params![nous_id, domain, window_size], |row| row.get(0))
            .context(error::CompetenceStoreSnafu {
                message: "failed to query rolling outcomes",
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context(error::CompetenceStoreSnafu {
                message: "failed to collect rolling outcomes",
            })?;

        let mut stats = RollingStats {
            window_size,
            total: u32::try_from(outcomes.len()).unwrap_or(u32::MAX),
            successes: 0,
            partials: 0,
            failures: 0,
        };

        for outcome_str in &outcomes {
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
    /// Escalation is recommended when the failure rate exceeds 30% with at
    /// least 5 recorded outcomes.
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

        let failure_rate = if stats.total >= ESCALATION_MIN_SAMPLES {
            f64::from(stats.failures) / f64::from(stats.total)
        } else {
            0.0
        };

        let should_escalate =
            stats.total >= ESCALATION_MIN_SAMPLES && failure_rate > ESCALATION_FAILURE_THRESHOLD;

        Ok(EscalationRecommendation {
            domain: domain.to_owned(),
            failure_rate,
            current_score,
            should_escalate,
        })
    }

    #[expect(
        clippy::disallowed_types,
        reason = "competence tracker owns its own isolated SQLite file; not part of the shared SessionStore pipeline"
    )]
    fn ensure_domain(
        conn: &Connection,
        nous_id: &str,
        domain: &str,
        now: &str,
    ) -> error::Result<()> {
        conn.execute(
            "INSERT OR IGNORE INTO competence_domains
                 (nous_id, domain, score, successes, partials, failures, corrections, disagreements, updated_at)
             VALUES (?1, ?2, ?3, 0, 0, 0, 0, 0, ?4)",
            params![nous_id, domain, DEFAULT_SCORE, now],
        )
        .context(error::CompetenceStoreSnafu {
            message: "failed to ensure domain row",
        })?;
        Ok(())
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

// WHY: rusqlite::OptionalExtension is needed for query_row().optional()
use rusqlite::OptionalExtension as _;

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests;
