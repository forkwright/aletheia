//! Server-side memory-health metrics (#4694).
//!
//! `theatron/proskenion` computes `avg_confidence`/`orphan_ratio`/
//! `staleness_ratio` client-side from a fact/entity list it already fetched
//! for other UI purposes (`views/meta/assembly.rs`). This computes the same
//! three components independently, server-side, from the knowledge store
//! directly, and feeds [`koina::memory_health::compute_health_score`] --
//! the same formula proskenion's `compute_health_score` now also delegates
//! to, so the two sides can disagree on their INPUTS (visibility scope at
//! query time, staleness-cutoff timing) but never on the calculation.
//!
//! Exported as Prometheus gauges via [`crate::metrics::update_memory_health_gauges`]
//! rather than kept purely internal, so the health trend is visible without
//! opening the TUI -- see `docs/OBSERVABILITY.md`'s Memory Health SLO section.

use std::sync::Arc;

use mneme::knowledge_store::KnowledgeStore;

use super::{count_orphaned_entities, count_relation};

/// Days since `recorded_at` before an active fact counts as stale.
///
/// WHY 30: matches `theatron/proskenion`'s `fact_is_stale` threshold
/// (`views/meta/assembly.rs`) exactly, so the two independently-computed
/// staleness ratios stay comparable.
const STALENESS_DAYS: i64 = 30;

const SECS_PER_DAY: i64 = 86_400;

/// The three inputs to [`koina::memory_health::compute_health_score`], plus
/// the resulting score, computed server-side.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct MemoryHealthMetrics {
    pub avg_confidence: f64,
    pub orphan_ratio: f64,
    pub staleness_ratio: f64,
    pub health_score: f64,
}

/// Compute memory-health metrics from the knowledge store.
///
/// # Errors
/// Returns an error message if the underlying Datalog queries fail.
pub(crate) fn compute_memory_health_metrics(
    store: &Arc<KnowledgeStore>,
) -> Result<MemoryHealthMetrics, String> {
    use std::collections::BTreeMap;

    // WHY this filter: mirrors proskenion's `active_facts` -- non-forgotten,
    // not superseded. `confidence`/`recorded_at`/`valid_to` are the only
    // columns staleness/confidence need; no entity join, unlike
    // load_entity_stats_from_store's per-entity fact_stats_script.
    let script = r"
        ?[confidence, recorded_at, valid_to] :=
            *facts{confidence, recorded_at, valid_to, is_forgotten, superseded_by},
            is_forgotten == false,
            is_null(superseded_by)
    ";
    let result = store
        .run_query(script, BTreeMap::new())
        .map_err(|e| format!("memory-health fact query failed: {e}"))?;

    let active_count = result.row_count();
    let now_secs = jiff::Timestamp::now().as_second();

    let mut confidence_sum = 0.0;
    let mut stale_count: usize = 0;
    for row in 0..active_count {
        confidence_sum += result.get_f64(row, "confidence").unwrap_or(0.0);
        let recorded_at = result.get_string(row, "recorded_at").unwrap_or_default();
        let valid_to = result.get_string(row, "valid_to").unwrap_or_default();
        if is_stale(&recorded_at, &valid_to, now_secs) {
            stale_count += 1;
        }
    }

    #[expect(
        clippy::as_conversions,
        clippy::cast_precision_loss,
        reason = "usize->f64: fact/entity counts are display-scale, never near f64's precision boundary"
    )]
    let (avg_confidence, staleness_ratio) = if active_count > 0 {
        (
            confidence_sum / active_count as f64,
            stale_count as f64 / active_count as f64,
        )
    } else {
        (0.0, 0.0)
    };

    let entity_count = count_relation(store, "entities")?;
    let orphaned_entity_count = count_orphaned_entities(store)?;
    #[expect(
        clippy::as_conversions,
        clippy::cast_precision_loss,
        reason = "usize->f64: fact/entity counts are display-scale, never near f64's precision boundary"
    )]
    let orphan_ratio = if entity_count > 0 {
        orphaned_entity_count as f64 / entity_count as f64
    } else {
        0.0
    };

    let health_score =
        koina::memory_health::compute_health_score(avg_confidence, orphan_ratio, staleness_ratio);

    Ok(MemoryHealthMetrics {
        avg_confidence,
        orphan_ratio,
        staleness_ratio,
        health_score,
    })
}

fn is_stale(recorded_at: &str, valid_to: &str, now_secs: i64) -> bool {
    let recorded_stale = recorded_at
        .parse::<jiff::Timestamp>()
        .is_ok_and(|ts| now_secs.saturating_sub(ts.as_second()) / SECS_PER_DAY > STALENESS_DAYS);
    let valid_to_past = valid_to
        .parse::<jiff::Timestamp>()
        .is_ok_and(|ts| ts.as_second() != 0 && ts.as_second() < now_secs);
    recorded_stale || valid_to_past
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iso_days_ago(days: i64) -> String {
        let ts = jiff::Timestamp::now() - jiff::SignedDuration::from_secs(days * SECS_PER_DAY);
        ts.to_string()
    }

    #[test]
    fn recent_fact_is_not_stale() {
        let now = jiff::Timestamp::now().as_second();
        assert!(!is_stale(&iso_days_ago(1), "", now));
    }

    #[test]
    fn fact_older_than_threshold_is_stale() {
        let now = jiff::Timestamp::now().as_second();
        assert!(is_stale(&iso_days_ago(STALENESS_DAYS + 1), "", now));
    }

    #[test]
    fn fact_at_exactly_threshold_is_not_yet_stale() {
        // WHY: mirrors proskenion's `d > 30` (strictly greater than), not `>=`.
        let now = jiff::Timestamp::now().as_second();
        assert!(!is_stale(&iso_days_ago(STALENESS_DAYS), "", now));
    }

    #[test]
    fn past_valid_to_makes_a_recent_fact_stale() {
        let now = jiff::Timestamp::now().as_second();
        assert!(is_stale(&iso_days_ago(1), &iso_days_ago(1), now));
    }

    #[test]
    fn empty_valid_to_does_not_count_as_past() {
        let now = jiff::Timestamp::now().as_second();
        assert!(!is_stale(&iso_days_ago(1), "", now));
    }

    #[test]
    fn unparseable_timestamps_do_not_panic_and_are_not_stale() {
        let now = jiff::Timestamp::now().as_second();
        assert!(!is_stale("not-a-timestamp", "also-not-one", now));
    }
}
