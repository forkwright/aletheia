//! `KnowledgeMaintenanceExecutor` implementation for the binary crate.
//!
//! Wires the daemon's maintenance trait to the concrete `KnowledgeStore`.
//! Three tasks are fully implemented; the remaining five log `NOT_IMPLEMENTED`
//! in their `detail` field pending future implementation (F.1-F.8).

use std::sync::Arc;

use aletheia_mneme::knowledge::FactType;
use aletheia_mneme::knowledge_store::KnowledgeStore;
use aletheia_mneme::recall::RecallEngine;
use aletheia_oikonomos::maintenance::{KnowledgeMaintenanceExecutor, MaintenanceReport};

/// Bridges the daemon's `KnowledgeMaintenanceExecutor` trait to the concrete
/// `KnowledgeStore`. All methods are blocking (`CozoDB` is sync).
pub(crate) struct KnowledgeMaintenanceAdapter {
    store: Arc<KnowledgeStore>,
}

impl KnowledgeMaintenanceAdapter {
    pub(crate) fn new(store: Arc<KnowledgeStore>) -> Self {
        Self { store }
    }
}

impl KnowledgeMaintenanceExecutor for KnowledgeMaintenanceAdapter {
    /// Query all current facts and apply FSRS decay via `RecallEngine::score_decay`.
    ///
    /// Updates confidence scores in place for each fact. Facts whose decay score
    /// has dropped more than 10% below their current confidence are updated.
    fn refresh_decay_scores(
        &self,
        nous_id: &str,
    ) -> aletheia_oikonomos::error::Result<MaintenanceReport> {
        let now = jiff::Timestamp::now();
        let now_str = aletheia_mneme::knowledge::format_timestamp(&now);

        let facts = self
            .store
            .query_facts(nous_id, &now_str, 10_000)
            .map_err(|e| {
                aletheia_oikonomos::error::TaskFailedSnafu {
                    task_id: "decay-refresh".to_owned(),
                    reason: e.to_string(),
                }
                .build()
            })?;

        let engine = RecallEngine::new();
        let mut items_processed: u64 = 0;
        let mut items_modified: u64 = 0;
        let mut errors: u32 = 0;

        for mut fact in facts {
            items_processed += 1;

            let reference_time = fact
                .access
                .last_accessed_at
                .unwrap_or(fact.temporal.recorded_at);
            let age_secs = now.duration_since(reference_time).as_secs();
            #[expect(
                clippy::cast_precision_loss,
                clippy::as_conversions,
                reason = "u64→f64: age in seconds is well within f64 precision for practical retention windows"
            )]
            let age_hours = (age_secs as f64 / 3600.0).max(0.0);

            let fact_type = FactType::from_str_lossy(&fact.fact_type);
            let decay_score = engine.score_decay(
                age_hours,
                fact_type,
                fact.provenance.tier,
                fact.access.access_count,
            );

            let new_confidence = (fact.provenance.confidence * decay_score).clamp(0.0, 1.0);
            if (fact.provenance.confidence - new_confidence).abs() > 0.01 {
                fact.provenance.confidence = new_confidence;
                if let Err(e) = self.store.insert_fact(&fact) {
                    tracing::warn!(
                        fact_id = %fact.id,
                        error = %e,
                        "decay refresh: failed to update fact confidence"
                    );
                    errors += 1;
                } else {
                    items_modified += 1;
                }
            }
        }

        let detail = format!(
            "Decay refresh: {items_processed} facts examined, {items_modified} confidence scores updated, {errors} errors"
        );
        tracing::info!(%detail, "maintenance: decay refresh complete");

        Ok(MaintenanceReport {
            items_processed,
            items_modified,
            errors,
            detail: Some(detail),
            ..Default::default()
        })
    }

    /// Deduplicates entities by delegating to `KnowledgeStore::run_entity_dedup`.
    fn deduplicate_entities(
        &self,
        nous_id: &str,
    ) -> aletheia_oikonomos::error::Result<MaintenanceReport> {
        let records = self.store.run_entity_dedup(nous_id).map_err(|e| {
            aletheia_oikonomos::error::TaskFailedSnafu {
                task_id: "entity-dedup".to_owned(),
                reason: e.to_string(),
            }
            .build()
        })?;

        #[expect(clippy::as_conversions, reason = "usize→u64: record count fits in u64")]
        let merged = records.len() as u64;
        let detail = format!("Entity dedup: {merged} entities merged automatically");
        tracing::info!(%detail, "maintenance: entity dedup complete");

        Ok(MaintenanceReport {
            items_processed: merged,
            items_modified: merged,
            detail: Some(detail),
            ..Default::default()
        })
    }

    fn recompute_graph_scores(
        &self,
        _nous_id: &str,
    ) -> aletheia_oikonomos::error::Result<MaintenanceReport> {
        Ok(MaintenanceReport {
            detail: Some(
                "NOT_IMPLEMENTED: graph score recomputation (PageRank/centrality) not yet wired"
                    .to_owned(),
            ),
            ..Default::default()
        })
    }

    /// Count facts without embeddings and log the gap.
    ///
    /// Cannot actually embed without an `EmbeddingProvider`, so this reports
    /// the count of current facts and notes that embedding refresh is not yet wired.
    fn refresh_embeddings(
        &self,
        nous_id: &str,
    ) -> aletheia_oikonomos::error::Result<MaintenanceReport> {
        let now = jiff::Timestamp::now();
        let now_str = aletheia_mneme::knowledge::format_timestamp(&now);
        let facts = self
            .store
            .query_facts(nous_id, &now_str, 10_000)
            .map_err(|e| {
                aletheia_oikonomos::error::TaskFailedSnafu {
                    task_id: "embedding-refresh".to_owned(),
                    reason: e.to_string(),
                }
                .build()
            })?;
        #[expect(clippy::as_conversions, reason = "usize→u64: fact count fits in u64")]
        let total_facts = facts.len() as u64;

        let detail = format!(
            "NOT_IMPLEMENTED: embedding refresh requires EmbeddingProvider — {total_facts} facts found, none re-embedded"
        );
        tracing::warn!(%detail, "maintenance: embedding refresh skipped");

        Ok(MaintenanceReport {
            items_processed: total_facts,
            items_modified: 0,
            detail: Some(detail),
            ..Default::default()
        })
    }

    fn garbage_collect(
        &self,
        _nous_id: &str,
    ) -> aletheia_oikonomos::error::Result<MaintenanceReport> {
        Ok(MaintenanceReport {
            detail: Some(
                "NOT_IMPLEMENTED: garbage collection of orphaned nodes/expired edges not yet wired"
                    .to_owned(),
            ),
            ..Default::default()
        })
    }

    fn maintain_indexes(
        &self,
        _nous_id: &str,
    ) -> aletheia_oikonomos::error::Result<MaintenanceReport> {
        Ok(MaintenanceReport {
            detail: Some("NOT_IMPLEMENTED: index rebuild/optimization not yet wired".to_owned()),
            ..Default::default()
        })
    }

    fn health_check(&self, _nous_id: &str) -> aletheia_oikonomos::error::Result<MaintenanceReport> {
        Ok(MaintenanceReport {
            detail: Some("NOT_IMPLEMENTED: knowledge graph health check not yet wired".to_owned()),
            ..Default::default()
        })
    }

    fn run_skill_decay(
        &self,
        nous_id: &str,
    ) -> aletheia_oikonomos::error::Result<MaintenanceReport> {
        let (active, needs_review, retired) = self.store.run_skill_decay(nous_id).map_err(|e| {
            aletheia_oikonomos::error::TaskFailedSnafu {
                task_id: "skill-decay".to_owned(),
                reason: e.to_string(),
            }
            .build()
        })?;

        let detail =
            format!("Skill decay: {active} active, {needs_review} needs_review, {retired} retired");
        tracing::info!(%detail, "maintenance: skill decay complete");

        #[expect(clippy::as_conversions, reason = "usize→u64: skill counts fit in u64")]
        Ok(MaintenanceReport {
            items_processed: (active + retired) as u64,
            items_modified: retired as u64,
            detail: Some(detail),
            ..Default::default()
        })
    }
}
