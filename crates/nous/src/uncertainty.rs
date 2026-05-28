// kanon:ignore RUST/file-too-long — uncertainty quantification with calibration curves; extraction planned in #3754
//! Uncertainty quantification: calibration of agent confidence estimates.
//!
//! Persisted in a fjall keyspace. Calibration points are kept per-agent and
//! pruned to a configured maximum window size per agent.
//!
//! # Key schema
//!
//! All keys are UTF-8 strings. Values are JSON-encoded [`CalibrationPoint`]s.
//!
//! | Partition      | Key pattern                                         | Value                    |
//! |----------------|-----------------------------------------------------|--------------------------|
//! | `points`       | `{nous_id}:{recorded_at}:{seq_padded_20}`           | JSON `CalibrationPoint`  |
//! | `counters`     | `{nous_id}`                                         | big-endian `u64`         |
//!
//! `recorded_at` (ISO 8601 lexicographic-sortable) + zero-padded sequence keeps
//! insertion order stable for pruning. `counters` provides a per-agent
//! monotonic sequence so concurrent records at the same timestamp don't
//! collide.

use std::path::Path;
use std::sync::{Arc, Mutex};

use fjall::{KeyspaceCreateOptions, SingleWriterTxDatabase};
use serde::{Deserialize, Serialize};

use crate::error;

/// Number of bins for the calibration curve (10 bins of width 0.1).
const NUM_BINS: usize = 10;

/// Bin width for calibration curve.
const BIN_WIDTH: f64 = 0.1;

/// Width for zero-padded sequence numbers.
const SEQ_WIDTH: usize = 20;

/// Partitions used by the uncertainty tracker.
const PARTITIONS: &[&str] = &["points", "counters"];

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

/// A single confidence prediction and its outcome.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CalibrationPoint {
    nous_id: String,
    domain: String,
    stated_confidence: f64,
    was_correct: bool,
    recorded_at: String,
}

/// Tracks agent confidence predictions vs actual outcomes.
pub(crate) struct UncertaintyTracker {
    db: Arc<SingleWriterTxDatabase>,
    /// Shared write mutex.
    write_lock: Mutex<()>,
    /// Kept alive to auto-delete the temp directory when the store is dropped.
    _temp_dir: Option<tempfile::TempDir>,
    /// Maximum stored calibration points per agent before oldest entries are pruned.
    max_calibration_points: u32,
}

impl UncertaintyTracker {
    /// Open a file-backed uncertainty tracker.
    ///
    /// # Errors
    ///
    /// Returns `UncertaintyStore` if the database cannot be opened or initialized.
    #[expect(
        dead_code,
        reason = "WIP: uncertainty calibration for agent confidence tracking — no callers yet, including tests"
    )]
    pub(crate) fn open(path: &Path) -> error::Result<Self> {
        let fdb = koina::fjall::FjallDb::open(path, PARTITIONS).map_err(|e| {
            error::UncertaintyStoreSnafu {
                message: format!("failed to open uncertainty database: {e}"),
            }
            .build()
        })?;
        Ok(Self::from_fjall_db(fdb, Self::default_max_points()))
    }

    /// Open an in-memory uncertainty tracker (for testing).
    ///
    /// The directory and all data are deleted when the returned tracker is
    /// dropped.
    ///
    /// # Errors
    ///
    /// Returns `UncertaintyStore` if the schema cannot be created.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "WIP: uncertainty calibration for agent confidence tracking"
        )
    )]
    pub(crate) fn open_in_memory() -> error::Result<Self> {
        let fdb = koina::fjall::FjallDb::open_temp(PARTITIONS).map_err(|e| {
            error::UncertaintyStoreSnafu {
                message: format!("failed to open in-memory uncertainty database: {e}"),
            }
            .build()
        })?;
        Ok(Self::from_fjall_db(fdb, Self::default_max_points()))
    }

    fn default_max_points() -> u32 {
        u32::try_from(
            taxis::config::AgentBehaviorDefaults::default().uncertainty_max_calibration_points,
        )
        .unwrap_or(1_000)
    }

    fn from_fjall_db(fdb: koina::fjall::FjallDb, max_calibration_points: u32) -> Self {
        Self {
            db: Arc::new(fdb.db),
            write_lock: fdb.write_lock,
            _temp_dir: fdb._temp_dir,
            max_calibration_points,
        }
    }

    fn partition(&self, name: &str) -> error::Result<fjall::SingleWriterTxKeyspace> {
        self.db
            .keyspace(name, KeyspaceCreateOptions::default)
            .map_err(|e| {
                error::UncertaintyStoreSnafu {
                    message: format!("fjall partition {name}: {e}"),
                }
                .build()
            })
    }

    fn next_seq(
        &self,
        counters: &fjall::SingleWriterTxKeyspace,
        nous_id: &str,
    ) -> error::Result<u64> {
        use fjall::Readable;
        let snap = self.db.read_tx();
        let current = snap
            .get(counters, nous_id.as_bytes())
            .map_err(|e| {
                error::UncertaintyStoreSnafu {
                    message: format!("fjall counter read: {e}"),
                }
                .build()
            })?
            .map_or(0, |b| decode_u64(&b));
        Ok(current + 1)
    }

    fn point_key(nous_id: &str, recorded_at: &str, seq: u64) -> String {
        format!("{nous_id}:{recorded_at}:{}", pad_u64(seq))
    }

    /// Record a confidence prediction and its actual outcome.
    ///
    /// The `stated_confidence` is clamped to \[0.0, 1.0\].
    ///
    /// # Errors
    ///
    /// Returns `UncertaintyStore` on database write failure.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "WIP: uncertainty calibration for agent confidence tracking"
        )
    )]
    pub(crate) fn record(
        &self,
        nous_id: &str,
        domain: &str,
        stated_confidence: f64,
        was_correct: bool,
    ) -> error::Result<()> {
        let clamped = stated_confidence.clamp(0.0, 1.0);
        let now = jiff::Timestamp::now().to_string();

        {
            let _guard = self
                .write_lock
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);

            let points = self.partition("points")?;
            let counters = self.partition("counters")?;

            let seq = self.next_seq(&counters, nous_id)?;

            let point = CalibrationPoint {
                nous_id: nous_id.to_owned(),
                domain: domain.to_owned(),
                stated_confidence: clamped,
                was_correct,
                recorded_at: now.clone(),
            };
            let data = serde_json::to_vec(&point).map_err(|e| {
                error::UncertaintyStoreSnafu {
                    message: format!("serialize calibration point: {e}"),
                }
                .build()
            })?;
            let key = Self::point_key(nous_id, &now, seq);

            let mut tx = self.db.write_tx();
            tx.insert(&points, key.as_str(), data.as_slice());
            tx.insert(&counters, nous_id, encode_u64(seq));
            tx.commit().map_err(|e| {
                error::UncertaintyStoreSnafu {
                    message: format!("fjall commit record: {e}"),
                }
                .build()
            })?;
        }

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
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "WIP: agent pipeline infrastructure")
    )]
    pub(crate) fn calibration_curve(
        &self,
        nous_id: Option<&str>,
    ) -> error::Result<Vec<CalibrationBin>> {
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
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "WIP: agent pipeline infrastructure")
    )]
    pub(crate) fn brier_score(&self, nous_id: Option<&str>) -> error::Result<f64> {
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
    #[expect(
        dead_code,
        reason = "WIP: agent pipeline infrastructure — no callers yet, including tests"
    )]
    pub(crate) fn ece(&self, nous_id: Option<&str>) -> error::Result<f64> {
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
    pub(crate) fn overconfidence_patterns(
        &self,
        nous_id: &str,
    ) -> error::Result<Vec<OverconfidencePattern>> {
        let points = self.load_points_with_domains(Some(nous_id))?;

        // Aggregate per domain.
        let mut per_domain: std::collections::BTreeMap<String, (f64, u32, u32)> =
            std::collections::BTreeMap::new();
        for point in &points {
            let entry = per_domain
                .entry(point.domain.clone())
                .or_insert((0.0, 0, 0));
            entry.0 += point.stated_confidence;
            entry.1 += u32::from(point.was_correct);
            entry.2 += 1;
        }

        let patterns: Vec<OverconfidencePattern> = per_domain
            .into_iter()
            .filter_map(|(domain, (conf_sum, correct_count, total))| {
                if total < 5 {
                    return None;
                }
                let avg_confidence = conf_sum / f64::from(total);
                let actual_rate = f64::from(correct_count) / f64::from(total);
                let gap = avg_confidence - actual_rate;
                Some(OverconfidencePattern {
                    domain,
                    avg_confidence,
                    actual_rate,
                    overconfidence_gap: gap,
                    sample_count: total,
                })
            })
            .filter(|p| p.overconfidence_gap > 0.15)
            .collect();

        Ok(patterns)
    }

    /// Get a full calibration summary for an agent.
    ///
    /// # Errors
    ///
    /// Returns `UncertaintyStore` on database read failure.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "WIP: uncertainty calibration for agent confidence tracking"
        )
    )]
    pub(crate) fn summary(&self, nous_id: &str) -> error::Result<CalibrationSummary> {
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

    fn load_points_with_domains(
        &self,
        nous_id: Option<&str>,
    ) -> error::Result<Vec<CalibrationPoint>> {
        use fjall::Readable;

        let points_part = self.partition("points")?;
        let snap = self.db.read_tx();

        // Collect all points matching the filter, sorted by key (which orders
        // by nous_id then recorded_at then seq — lexicographic).
        let mut raw: Vec<(Vec<u8>, CalibrationPoint)> = Vec::new();
        let iter: Box<dyn Iterator<Item = _>> = match nous_id {
            Some(id) => {
                let prefix = format!("{id}:");
                let upper = format!("{id};\x00");
                Box::new(snap.range(&points_part, prefix..upper))
            }
            None => Box::new(snap.range::<&str, _>(&points_part, ..)),
        };

        for guard in iter {
            if let Ok((k, v)) = guard.into_inner()
                && let Ok(point) = serde_json::from_slice::<CalibrationPoint>(&v)
            {
                raw.push((k.to_vec(), point));
            }
        }

        // Sort by key (ascending: oldest first); limit keeps the most recent.
        raw.sort_by(|a, b| a.0.cmp(&b.0));

        // Apply per-agent limit.
        let limit = usize::try_from(self.max_calibration_points).unwrap_or(usize::MAX);
        let mut points: Vec<CalibrationPoint> = if nous_id.is_some() {
            if raw.len() > limit {
                let start = raw.len() - limit;
                raw.split_off(start).into_iter().map(|(_, p)| p).collect()
            } else {
                raw.into_iter().map(|(_, p)| p).collect()
            }
        } else {
            // For the "all agents" view, apply the same cap to keep bounded work.
            if raw.len() > limit {
                let start = raw.len() - limit;
                raw.split_off(start).into_iter().map(|(_, p)| p).collect()
            } else {
                raw.into_iter().map(|(_, p)| p).collect()
            }
        };

        // Return most-recent-first to match the SQL `ORDER BY recorded_at DESC` contract.
        points.reverse();
        Ok(points)
    }

    fn load_points(&self, nous_id: Option<&str>) -> error::Result<Vec<(f64, bool)>> {
        let points = self.load_points_with_domains(nous_id)?;
        Ok(points
            .into_iter()
            .map(|p| (p.stated_confidence, p.was_correct))
            .collect())
    }

    fn prune_old_points(&self, nous_id: &str) -> error::Result<()> {
        use fjall::Readable;

        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let points_part = self.partition("points")?;
        let prefix = format!("{nous_id}:");
        let upper = format!("{nous_id};\x00");

        let snap = self.db.read_tx();
        let mut keys: Vec<Vec<u8>> = Vec::new();
        for guard in snap.range(&points_part, prefix.as_str()..upper.as_str()) {
            if let Ok((k, _)) = guard.into_inner() {
                keys.push(k.to_vec());
            }
        }
        drop(snap);

        keys.sort();

        let limit = usize::try_from(self.max_calibration_points).unwrap_or(usize::MAX);
        if keys.len() <= limit {
            return Ok(());
        }
        let to_remove = keys.len() - limit;

        let mut tx = self.db.write_tx();
        for key in keys.iter().take(to_remove) {
            tx.remove(&points_part, key.as_slice());
        }
        tx.commit().map_err(|e| {
            error::UncertaintyStoreSnafu {
                message: format!("fjall prune: {e}"),
            }
            .build()
        })?;

        Ok(())
    }
}

fn compute_calibration_curve(points: &[(f64, bool)]) -> Vec<CalibrationBin> {
    let mut bins = Vec::with_capacity(NUM_BINS);

    for i in 0..NUM_BINS {
        // WHY f64::from(u32): bin index is bounded by NUM_BINS (10), so
        // `try_from` is infallible in practice; u32→f64 is an exact
        // conversion (u32 values fit in f64 mantissa exactly).
        let i_u32 = u32::try_from(i).unwrap_or(u32::MAX);
        let low = f64::from(i_u32) * BIN_WIDTH;
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

    // WHY f64::from(u32): calibration point count is bounded by
    // MAX_CALIBRATION_POINTS (1000), so `try_from` is infallible in
    // practice; u32→f64 is an exact conversion.
    let count_u32 = u32::try_from(points.len()).unwrap_or(u32::MAX);
    let count = f64::from(count_u32);
    sum / count
}

fn compute_ece(curve: &[CalibrationBin]) -> f64 {
    let mut weighted_error = 0.0;
    let mut total_points = 0u32;

    for bin in curve {
        if bin.total == 0 {
            continue;
        }
        let midpoint = f64::midpoint(bin.range.0, bin.range.1);
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
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::as_conversions,
        reason = "test data: NUM_BINS=10 bin index to f64 is exact, and midpoint*10.0 is a whole number 0..10 by construction (non-negative, fits usize)"
    )]
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
        let max =
            taxis::config::AgentBehaviorDefaults::default().uncertainty_max_calibration_points;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::as_conversions,
            reason = "test value fits in u32"
        )]
        let limit = (max + 10) as u32;
        for i in 0..limit {
            t.record("syn", "coding", 0.5, i % 2 == 0).unwrap();
        }

        let points = t.load_points(Some("syn")).unwrap();
        assert!(
            points.len() <= max,
            "should prune to at most {max} points, got {}",
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
