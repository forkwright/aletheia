//! `KnowledgeMaintenanceExecutor` implementation for the binary crate.
//!
//! Wires the daemon's maintenance trait to the concrete `KnowledgeStore`.
//!
//! Only tasks backed by concrete `KnowledgeStore` behavior report success.

use std::sync::Arc;

use mneme::knowledge::FactType;
use mneme::knowledge_store::KnowledgeStore;
use mneme::recall::RecallEngine;
use oikonomos::maintenance::{KnowledgeMaintenanceExecutor, MaintenanceReport};

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
    fn insert_fact(&self, fact: &mneme::knowledge::Fact) -> oikonomos::error::Result<()> {
        self.store.insert_fact(fact).map_err(|e| {
            oikonomos::error::TaskFailedSnafu {
                task_id: "fact-persistence".to_owned(),
                reason: e.to_string(),
            }
            .build()
        })
    }

    /// Query all current facts and apply FSRS decay via `RecallEngine::score_decay`.
    ///
    /// Updates confidence scores in place for each fact. Facts whose decay score
    /// has dropped more than 10% below their current confidence are updated.
    fn refresh_decay_scores(&self, nous_id: &str) -> oikonomos::error::Result<MaintenanceReport> {
        let now = jiff::Timestamp::now();
        let now_str = mneme::knowledge::format_timestamp(&now);

        let facts = self
            .store
            .query_facts(nous_id, &now_str, 10_000)
            .map_err(|e| {
                oikonomos::error::TaskFailedSnafu {
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
    fn deduplicate_entities(&self, nous_id: &str) -> oikonomos::error::Result<MaintenanceReport> {
        let records = self.store.run_entity_dedup(nous_id).map_err(|e| {
            oikonomos::error::TaskFailedSnafu {
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
    ) -> oikonomos::error::Result<MaintenanceReport> {
        let start = std::time::Instant::now();
        let mut items_modified: u64 = 0;
        let mut errors: u32 = 0;

        if let Err(e) = self.store.recompute_graph_scores() {
            tracing::warn!(error = %e, "graph score recomputation failed");
            errors += 1;
        } else {
            items_modified += 1;
        }

        if let Err(e) = self.store.compute_and_store_volatility() {
            tracing::warn!(error = %e, "volatility score computation failed");
            errors += 1;
        } else {
            items_modified += 1;
        }

        let duration_ms = start.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
        let detail = format!(
            "Graph recompute: PageRank+Louvain refreshed, volatility stored, {errors} errors"
        );
        tracing::info!(%detail, duration_ms, "maintenance: graph recompute complete");

        Ok(MaintenanceReport {
            items_processed: items_modified,
            items_modified,
            errors,
            duration_ms,
            detail: Some(detail),
        })
    }

    fn refresh_embeddings(&self, _nous_id: &str) -> oikonomos::error::Result<MaintenanceReport> {
        Err(oikonomos::error::TaskFailedSnafu {
            task_id: "embedding-refresh".to_owned(),
            reason: "embedding refresh requires an EmbeddingProvider bridge and is not scheduled"
                .to_owned(),
        }
        .build())
    }

    fn garbage_collect(&self, _nous_id: &str) -> oikonomos::error::Result<MaintenanceReport> {
        Err(oikonomos::error::TaskFailedSnafu {
            task_id: "knowledge-gc".to_owned(),
            reason:
                "knowledge garbage collection has no concrete store contract and is not scheduled"
                    .to_owned(),
        }
        .build())
    }

    fn maintain_indexes(&self, _nous_id: &str) -> oikonomos::error::Result<MaintenanceReport> {
        Err(oikonomos::error::TaskFailedSnafu {
            task_id: "index-maintenance".to_owned(),
            reason: "index maintenance has no concrete store contract and is not scheduled"
                .to_owned(),
        }
        .build())
    }

    fn health_check(&self, _nous_id: &str) -> oikonomos::error::Result<MaintenanceReport> {
        Err(oikonomos::error::TaskFailedSnafu {
            task_id: "graph-health-check".to_owned(),
            reason: "knowledge graph health check has no concrete diagnostic contract and is not scheduled"
                .to_owned(),
        }
        .build())
    }

    fn run_skill_decay(&self, nous_id: &str) -> oikonomos::error::Result<MaintenanceReport> {
        let (active, needs_review, retired) = self.store.run_skill_decay(nous_id).map_err(|e| {
            oikonomos::error::TaskFailedSnafu {
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
