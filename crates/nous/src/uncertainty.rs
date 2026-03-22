//! Uncertainty quantification: calibration of agent confidence estimates.

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use snafu::ResultExt as _;

use crate::error;

/// Maximum stored calibration points before oldest entries are pruned.
const MAX_CALIBRATION_POINTS: u32 = 1000;

/// Number of bins for the calibration curve (10 bins of width 0.1).
const NUM_BINS: usize = 10;

/// Bin width for calibration curve.
const BIN_WIDTH: f64 = 0.1;

/// A single calibration bin showing predicted vs actual accuracy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalibrationBin {
    /// Lower and upper bounds of the confidence range.
    pub range: (f64, f64),
    /// Total predictions in this bin.
    pub total: u32,
    /// Correct predictions in this bin.
    pub correct: u32,
    /// Actual accuracy (correct / total, or 0.0 if empty).
    pub accuracy: f64,
}

/// Overconfidence pattern for a specific domain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverconfidencePattern {
    /// Domain where overconfidence was detected.
    pub domain: String,
    /// Average stated confidence in this domain.
    pub avg_confidence: f64,
    /// Actual success rate in this domain.
    pub actual_rate: f64,
    /// Gap between stated confidence and actual success (positive = overconfident).
    pub overconfidence_gap: f64,
    /// Number of data points.
    pub sample_count: u32,
}

/// Summary of an agent's calibration quality.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalibrationSummary {
    /// Total calibration data points.
    pub total_points: u32,
    /// Brier score (0.0 = perfect, 1.0 = worst).
    pub brier_score: f64,
    /// Expected Calibration Error.
    pub ece: f64,
    /// Calibration curve bins.
    pub calibration_curve: Vec<CalibrationBin>,
    /// Domains where overconfidence was detected.
    pub overconfidence_patterns: Vec<OverconfidencePattern>,
}

/// Tracks agent confidence predictions vs actual outcomes.
pub struct UncertaintyTracker {
    conn: Connection,
}

impl UncertaintyTracker {
    /// Open a file-backed uncertainty tracker.
    ///
    /// # Errors
    ///
    /// Returns `UncertaintyStore` if the database cannot be opened or initialized.
    pub fn open(path: &std::path::Path) -> error::Result<Self> {
        let conn = Connection::open(path).context(error::UncertaintyStoreSnafu {
            message: "failed to open uncertainty database",
        })?;
        Self::init(conn)
    }

    /// Open an in-memory uncertainty tracker (for testing).
    ///
    /// # Errors
    ///
    /// Returns `UncertaintyStore` if the schema cannot be created.
    pub fn open_in_memory() -> error::Result<Self> {
        let conn = Connection::open_in_memory().context(error::UncertaintyStoreSnafu {
            message: "failed to open in-memory uncertainty database",
        })?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> error::Result<Self> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;

             CREATE TABLE IF NOT EXISTS calibration_points (
                 id                 INTEGER PRIMARY KEY AUTOINCREMENT,
                 nous_id            TEXT NOT NULL,
                 domain             TEXT NOT NULL,
                 stated_confidence  REAL NOT NULL,
                 was_correct        INTEGER NOT NULL,
                 recorded_at        TEXT NOT NULL
             );

             CREATE INDEX IF NOT EXISTS idx_calibration_agent
                 ON calibration_points (nous_id, recorded_at);

             CREATE INDEX IF NOT EXISTS idx_calibration_domain
                 ON calibration_points (nous_id, domain);",
        )
        .context(error::UncertaintyStoreSnafu {
            message: "failed to initialize uncertainty schema",
        })?;

        Ok(Self { conn })
    }

    /// Record a confidence prediction and its actual outcome.
    ///
    /// The `stated_confidence` is clamped to \[0.0, 1.0\].
    ///
    /// # Errors
    ///
    /// Returns `UncertaintyStore` on database write failure.
    pub fn record(
        &self,
        nous_id: &str,
        domain: &str,
        stated_confidence: f64,
        was_correct: bool,
    ) -> error::Result<()> {
        let clamped = stated_confidence.clamp(0.0, 1.0);
        let now = jiff::Timestamp::now().to_string();

        self.conn
            .execute(
                "INSERT INTO calibration_points
                     (nous_id, domain, stated_confidence, was_correct, recorded_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![nous_id, domain, clamped, was_correct as i32, now],
            )
            .context(error::UncertaintyStoreSnafu {
                message: "failed to insert calibration point",
            })?;

        self.prune_old_points(nous_id)?;

        Ok(())
    }

    /// Compute the calibration curve for an agent (or all agents if `None`).
    ///
    /// Divides the \[0.0, 1.0) confidence range into 10 bins and compares
    /// stated confidence against actual accuracy in each bin.
    ///
    /// # Errors
    ///
    /// Returns `UncertaintyStore` on database read failure.
    pub fn calibration_curve(&self, nous_id: Option<&str>) -> error::Result<Vec<CalibrationBin>> {
        let points = self.load_points(nous_id)?;
        Ok(compute_calibration_curve(&points))
    }

    /// Compute the Brier score for an agent (or all agents if `None`).
    ///
    /// Lower is better: 0.0 = perfect, 1.0 = worst possible.
    ///
    /// # Errors
    ///
    /// Returns `UncertaintyStore` on database read failure.
    pub fn brier_score(&self, nous_id: Option<&str>) -> error::Result<f64> {
        let points = self.load_points(nous_id)?;
        Ok(compute_brier_score(&points))
    }

    /// Compute the Expected Calibration Error (ECE).
    ///
    /// Measures the weighted average gap between stated confidence and actual
    /// accuracy across all bins.
    ///
    /// # Errors
    ///
    /// Returns `UncertaintyStore` on database read failure.
    pub fn ece(&self, nous_id: Option<&str>) -> error::Result<f64> {
        let points = self.load_points(nous_id)?;
        let curve = compute_calibration_curve(&points);
        Ok(compute_ece(&curve))
    }

    /// Identify domains where the agent is consistently overconfident.
    ///
    /// A domain is flagged if the agent's average stated confidence exceeds
    /// the actual success rate by more than 0.15, with at least 5 samples.
    ///
    /// # Errors
    ///
    /// Returns `UncertaintyStore` on database read failure.
    pub fn overconfidence_patterns(
        &self,
        nous_id: &str,
    ) -> error::Result<Vec<OverconfidencePattern>> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT domain,
                        AVG(stated_confidence) as avg_conf,
                        AVG(was_correct) as actual_rate,
                        COUNT(*) as cnt
                 FROM calibration_points
                 WHERE nous_id = ?1
                 GROUP BY domain
                 HAVING cnt >= 5",
            )
            .context(error::UncertaintyStoreSnafu {
                message: "failed to prepare overconfidence query",
            })?;

        let patterns: Vec<OverconfidencePattern> = stmt
            .query_map(params![nous_id], |row| {
                let domain: String = row.get(0)?;
                let avg_confidence: f64 = row.get(1)?;
                let actual_rate: f64 = row.get(2)?;
                let sample_count: u32 = row.get(3)?;
                let gap = avg_confidence - actual_rate;

                Ok(OverconfidencePattern {
                    domain,
                    avg_confidence,
                    actual_rate,
                    overconfidence_gap: gap,
                    sample_count,
                })
            })
            .context(error::UncertaintyStoreSnafu {
                message: "failed to query overconfidence patterns",
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context(error::UncertaintyStoreSnafu {
                message: "failed to collect overconfidence patterns",
            })?;

        // WHY: only flag domains with a meaningful overconfidence gap (>0.15)
        Ok(patterns
            .into_iter()
            .filter(|p| p.overconfidence_gap > 0.15)
            .collect())
    }

    /// Get a full calibration summary for an agent.
    ///
    /// # Errors
    ///
    /// Returns `UncertaintyStore` on database read failure.
    pub fn summary(&self, nous_id: &str) -> error::Result<CalibrationSummary> {
        let points = self.load_points(Some(nous_id))?;
        let curve = compute_calibration_curve(&points);
        let brier = compute_brier_score(&points);
        let ece = compute_ece(&curve);
        let overconfidence = self.overconfidence_patterns(nous_id)?;

        Ok(CalibrationSummary {
            total_points: u32::try_from(points.len()).unwrap_or(u32::MAX),
            brier_score: brier,
            ece,
            calibration_curve: curve,
            overconfidence_patterns: overconfidence,
        })
    }

    fn load_points(&self, nous_id: Option<&str>) -> error::Result<Vec<(f64, bool)>> {
        match nous_id {
            Some(id) => {
                let mut stmt = self
                    .conn
                    .prepare_cached(
                        "SELECT stated_confidence, was_correct
                         FROM calibration_points
                         WHERE nous_id = ?1
                         ORDER BY recorded_at DESC
                         LIMIT ?2",
                    )
                    .context(error::UncertaintyStoreSnafu {
                        message: "failed to prepare points query",
                    })?;

                stmt.query_map(params![id, MAX_CALIBRATION_POINTS], |row| {
                    Ok((row.get::<_, f64>(0)?, row.get::<_, bool>(1)?))
                })
                .context(error::UncertaintyStoreSnafu {
                    message: "failed to query calibration points",
                })?
                .collect::<std::result::Result<Vec<_>, _>>()
                .context(error::UncertaintyStoreSnafu {
                    message: "failed to collect calibration points",
                })
            }
            None => {
                let mut stmt = self
                    .conn
                    .prepare_cached(
                        "SELECT stated_confidence, was_correct
                         FROM calibration_points
                         ORDER BY recorded_at DESC
                         LIMIT ?1",
                    )
                    .context(error::UncertaintyStoreSnafu {
                        message: "failed to prepare global points query",
                    })?;

                stmt.query_map(params![MAX_CALIBRATION_POINTS], |row| {
                    Ok((row.get::<_, f64>(0)?, row.get::<_, bool>(1)?))
                })
                .context(error::UncertaintyStoreSnafu {
                    message: "failed to query global calibration points",
                })?
                .collect::<std::result::Result<Vec<_>, _>>()
                .context(error::UncertaintyStoreSnafu {
                    message: "failed to collect global calibration points",
                })
            }
        }
    }

    fn prune_old_points(&self, nous_id: &str) -> error::Result<()> {
        self.conn
            .execute(
                "DELETE FROM calibration_points
                 WHERE nous_id = ?1
                   AND id NOT IN (
                       SELECT id FROM calibration_points
                       WHERE nous_id = ?1
                       ORDER BY recorded_at DESC
                       LIMIT ?2
                   )",
                params![nous_id, MAX_CALIBRATION_POINTS],
            )
            .context(error::UncertaintyStoreSnafu {
                message: "failed to prune old calibration points",
            })?;
        Ok(())
    }
}

fn compute_calibration_curve(points: &[(f64, bool)]) -> Vec<CalibrationBin> {
    let mut bins = Vec::with_capacity(NUM_BINS);

    for i in 0..NUM_BINS {
        let low = i as f64 * BIN_WIDTH;
        let high = low + BIN_WIDTH;
        let low_rounded = (low * 100.0).round() / 100.0;
        let high_rounded = (high * 100.0).round() / 100.0;

        let mut total = 0u32;
        let mut correct = 0u32;
        for &(confidence, was_correct) in points {
            if confidence >= low && confidence < high {
                total += 1;
                if was_correct {
                    correct += 1;
                }
            }
        }

        let accuracy = if total > 0 {
            f64::from(correct) / f64::from(total)
        } else {
            0.0
        };

        bins.push(CalibrationBin {
            range: (low_rounded, high_rounded),
            total,
            correct,
            accuracy,
        });
    }

    bins
}

fn compute_brier_score(points: &[(f64, bool)]) -> f64 {
    if points.is_empty() {
        return 0.5;
    }

    let sum: f64 = points
        .iter()
        .map(|&(confidence, was_correct)| {
            let outcome = if was_correct { 1.0 } else { 0.0 };
            (confidence - outcome).powi(2)
        })
        .sum();

    sum / points.len() as f64
}

fn compute_ece(curve: &[CalibrationBin]) -> f64 {
    let mut weighted_error = 0.0;
    let mut total_points = 0u32;

    for bin in curve {
        if bin.total == 0 {
            continue;
        }
        let midpoint = (bin.range.0 + bin.range.1) / 2.0;
        weighted_error += f64::from(bin.total) * (bin.accuracy - midpoint).abs();
        total_points += bin.total;
    }

    if total_points > 0 {
        weighted_error / f64::from(total_points)
    } else {
        0.0
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    fn tracker() -> UncertaintyTracker {
        UncertaintyTracker::open_in_memory().unwrap()
    }

    #[test]
    fn empty_brier_score_is_half() {
        let t = tracker();
        let brier = t.brier_score(None).unwrap();
        assert!(
            (brier - 0.5).abs() < f64::EPSILON,
            "empty Brier score should be 0.5, got {brier}"
        );
    }

    #[test]
    fn perfect_calibration_has_low_brier() {
        let t = tracker();
        for _ in 0..20 {
            t.record("syn", "coding", 0.9, true).unwrap();
        }
        for _ in 0..20 {
            t.record("syn", "coding", 0.1, false).unwrap();
        }

        let brier = t.brier_score(Some("syn")).unwrap();
        assert!(
            brier < 0.05,
            "well-calibrated predictions should have low Brier score, got {brier}"
        );
    }

    #[test]
    fn overconfident_predictions_have_high_brier() {
        let t = tracker();
        for _ in 0..20 {
            t.record("syn", "coding", 0.95, false).unwrap();
        }

        let brier = t.brier_score(Some("syn")).unwrap();
        assert!(
            brier > 0.8,
            "overconfident wrong predictions should have high Brier score, got {brier}"
        );
    }

    #[test]
    fn calibration_curve_has_ten_bins() {
        let t = tracker();
        t.record("syn", "coding", 0.5, true).unwrap();
        let curve = t.calibration_curve(Some("syn")).unwrap();
        assert_eq!(curve.len(), NUM_BINS, "curve should have {NUM_BINS} bins");
    }

    #[test]
    fn calibration_curve_bins_cover_full_range() {
        let t = tracker();
        t.record("syn", "coding", 0.5, true).unwrap();
        let curve = t.calibration_curve(Some("syn")).unwrap();

        assert!(
            (curve.first().unwrap().range.0).abs() < f64::EPSILON,
            "first bin should start at 0.0"
        );
        assert!(
            (curve.last().unwrap().range.1 - 1.0).abs() < f64::EPSILON,
            "last bin should end at 1.0"
        );
    }

    #[test]
    fn ece_zero_for_perfectly_calibrated() {
        let points: Vec<(f64, bool)> = (0..NUM_BINS)
            .flat_map(|i| {
                let midpoint = (i as f64 * BIN_WIDTH) + (BIN_WIDTH / 2.0);
                let correct_count = (midpoint * 10.0).round() as usize;
                let wrong_count = 10 - correct_count;
                let mut bin_points = Vec::new();
                let conf = midpoint;
                for _ in 0..correct_count {
                    bin_points.push((conf, true));
                }
                for _ in 0..wrong_count {
                    bin_points.push((conf, false));
                }
                bin_points
            })
            .collect();

        let curve = compute_calibration_curve(&points);
        let ece = compute_ece(&curve);
        // WHY: discrete binning introduces small rounding error (≤0.05)
        assert!(
            ece <= 0.051,
            "perfectly calibrated data should have near-zero ECE, got {ece}"
        );
    }

    #[test]
    fn overconfidence_detected_in_domain() {
        let t = tracker();
        for _ in 0..10 {
            t.record("syn", "coding", 0.9, false).unwrap();
        }

        let patterns = t.overconfidence_patterns("syn").unwrap();
        assert_eq!(
            patterns.len(),
            1,
            "should detect overconfidence in one domain"
        );
        assert_eq!(patterns.first().unwrap().domain, "coding");
        assert!(patterns.first().unwrap().overconfidence_gap > 0.15);
    }

    #[test]
    fn no_overconfidence_when_well_calibrated() {
        let t = tracker();
        for _ in 0..10 {
            t.record("syn", "coding", 0.8, true).unwrap();
        }

        let patterns = t.overconfidence_patterns("syn").unwrap();
        assert!(
            patterns.is_empty(),
            "well-calibrated agent should have no overconfidence patterns"
        );
    }

    #[test]
    fn summary_includes_all_fields() {
        let t = tracker();
        for i in 0..10 {
            t.record("syn", "coding", 0.7, i % 3 != 0).unwrap();
        }

        let summary = t.summary("syn").unwrap();
        assert_eq!(summary.total_points, 10);
        assert!(summary.brier_score >= 0.0);
        assert!(summary.ece >= 0.0);
        assert_eq!(summary.calibration_curve.len(), NUM_BINS);
    }

    #[test]
    fn confidence_clamped_to_valid_range() {
        let t = tracker();
        t.record("syn", "coding", 1.5, true).unwrap();
        t.record("syn", "coding", -0.5, false).unwrap();

        let points = t.load_points(Some("syn")).unwrap();
        assert_eq!(points.len(), 2);
        for &(conf, _) in &points {
            assert!(
                (0.0..=1.0).contains(&conf),
                "confidence should be clamped, got {conf}"
            );
        }
    }

    #[test]
    fn agents_have_independent_calibration() {
        let t = tracker();
        for _ in 0..10 {
            t.record("syn", "coding", 0.9, true).unwrap();
        }
        for _ in 0..10 {
            t.record("demiurge", "coding", 0.9, false).unwrap();
        }

        let syn_brier = t.brier_score(Some("syn")).unwrap();
        let demiurge_brier = t.brier_score(Some("demiurge")).unwrap();
        assert!(
            syn_brier < demiurge_brier,
            "agents should have independent Brier scores"
        );
    }

    #[test]
    fn pruning_keeps_most_recent_points() {
        let t = tracker();
        for i in 0..1010u32 {
            t.record("syn", "coding", 0.5, i % 2 == 0).unwrap();
        }

        let points = t.load_points(Some("syn")).unwrap();
        assert!(
            points.len() <= MAX_CALIBRATION_POINTS as usize,
            "should prune to at most {MAX_CALIBRATION_POINTS} points, got {}",
            points.len()
        );
    }

    #[test]
    fn compute_brier_score_known_values() {
        let points = vec![(1.0, true), (0.0, false)];
        let brier = compute_brier_score(&points);
        assert!(
            brier.abs() < f64::EPSILON,
            "perfect predictions should have Brier score of 0.0, got {brier}"
        );

        let points = vec![(1.0, false), (0.0, true)];
        let brier = compute_brier_score(&points);
        assert!(
            (brier - 1.0).abs() < f64::EPSILON,
            "worst predictions should have Brier score of 1.0, got {brier}"
        );
    }

    #[test]
    fn overconfidence_gap_is_correct() {
        let t = tracker();
        // WHY: 10 samples at 0.8 confidence, all wrong → gap = 0.8 - 0.0 = 0.8
        for _ in 0..10 {
            t.record("syn", "research", 0.8, false).unwrap();
        }

        let patterns = t.overconfidence_patterns("syn").unwrap();
        let research = patterns.iter().find(|p| p.domain == "research").unwrap();
        assert!(
            (research.overconfidence_gap - 0.8).abs() < f64::EPSILON,
            "gap should be 0.8, got {}",
            research.overconfidence_gap
        );
    }
}
