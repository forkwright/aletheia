//! `KnowledgeMaintenanceExecutor` implementation for the binary crate.
//!
//! Wires the daemon's maintenance trait to the concrete `KnowledgeStore`.
//!
//! Only tasks backed by concrete `KnowledgeStore` behavior report success.

use std::path::PathBuf;
use std::sync::Arc;

use episteme::consolidation::{ConsolidationConfig, ConsolidationProvider};
use hermeneus::provider::ProviderRegistry;
use hermeneus::types::{CompletionRequest, Content, ContentBlock, Message, Role};
use mneme::dedup::DedupTuning;
use mneme::embedding::{EmbeddingProvider, is_degraded_provider};
use mneme::knowledge::FactType;
use mneme::knowledge_store::KnowledgeStore;
use mneme::recall::RecallEngine;
use oikonomos::maintenance::{KnowledgeMaintenanceExecutor, MaintenanceReport};
use taxis::config::AgentBehaviorDefaults;

/// Bridges the daemon's `KnowledgeMaintenanceExecutor` trait to the concrete
/// `KnowledgeStore`. All methods are blocking because Krites query execution is sync.
pub(crate) struct KnowledgeMaintenanceAdapter {
    store: Arc<KnowledgeStore>,
    /// Embedding provider passed through to the dedup pipeline so it can
    /// populate `entities.name_embedding` before scoring (#4165 Path A).
    /// `None` (or a degraded sentinel) keeps `embed_sim = 0.0`, which
    /// preserves pre-fix behaviour for installs without an embedding
    /// provider configured.
    embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    /// Operator-configurable dedup weights and thresholds (#4165 D).
    /// Defaults to [`DedupTuning::DEFAULT`] when the runtime does not
    /// override it; production startup builds one from
    /// `AgentBehaviorDefaults::knowledge_dedup_*` via
    /// [`tuning_from_behavior`] so config knobs actually take effect.
    tuning: DedupTuning,
    /// LLM bridge for the knowledge consolidation engine (#5530).
    ///
    /// `None` leaves consolidation as a no-op so the maintenance task can
    /// still be scheduled in deployments without a configured LLM provider.
    consolidation_provider: Option<Arc<dyn ConsolidationProvider>>,
}

/// Build a [`DedupTuning`] from the resolved `AgentBehaviorDefaults` so
/// the runtime can hand the maintenance task / CLI a single struct that
/// reflects every operator-configured `knowledge_dedup_*` key.
///
/// `auto_merge_threshold` and `review_threshold` currently fall back to
/// [`DedupTuning::DEFAULT`] — the config struct does not (yet) carry
/// keys for them. Adding those keys is a strict superset change deferred
/// out of #4165's mechanical bundle (filed for a future PR).
pub(crate) fn tuning_from_behavior(defaults: &AgentBehaviorDefaults) -> DedupTuning {
    DedupTuning {
        weight_name: defaults.knowledge_dedup_weight_name,
        weight_embed: defaults.knowledge_dedup_weight_embed,
        weight_type: defaults.knowledge_dedup_weight_type,
        weight_alias: defaults.knowledge_dedup_weight_alias,
        jw_threshold: defaults.knowledge_dedup_jw_threshold,
        embed_threshold: defaults.knowledge_dedup_embed_threshold,
        auto_merge_threshold: DedupTuning::DEFAULT.auto_merge_threshold,
        review_threshold: DedupTuning::DEFAULT.review_threshold,
    }
}

impl KnowledgeMaintenanceAdapter {
    pub(crate) fn new(store: Arc<KnowledgeStore>) -> Self {
        Self {
            store,
            embedding_provider: None,
            tuning: DedupTuning::DEFAULT,
            consolidation_provider: None,
        }
    }

    /// Attach an embedding provider to the dedup pipeline.
    ///
    /// When set, [`Self::deduplicate_entities`] backfills any NULL
    /// `entities.name_embedding`s through this provider before running
    /// the merge-score pipeline, so the 0.30-weighted `embed_sim` term
    /// becomes a real signal and the `AutoMerge` threshold (0.90) is
    /// reachable. A degraded sentinel is accepted but skipped at
    /// backfill time to avoid log spam.
    pub(crate) fn with_embedding_provider(mut self, provider: Arc<dyn EmbeddingProvider>) -> Self {
        self.embedding_provider = Some(provider);
        self
    }

    /// Override the default dedup tuning (#4165 D).
    ///
    /// Production startup calls [`tuning_from_behavior`] against the
    /// resolved `AgentBehaviorDefaults` and passes the result here so the
    /// scheduled maintenance task honours the operator's
    /// `knowledge_dedup_*` config keys. Tests and degraded installs that
    /// skip this call get [`DedupTuning::DEFAULT`].
    pub(crate) fn with_tuning(mut self, tuning: DedupTuning) -> Self {
        self.tuning = tuning;
        self
    }

    /// Attach an LLM bridge for knowledge consolidation (#5530).
    ///
    /// When set, [`KnowledgeMaintenanceExecutor::consolidate_knowledge`]
    /// delegates to the engine in `crates/episteme`. When unset,
    /// consolidation reports success without doing work, matching the
    /// pre-wiring behaviour for installs without a provider.
    pub(crate) fn with_consolidation_provider(
        mut self,
        provider: Arc<dyn ConsolidationProvider>,
    ) -> Self {
        self.consolidation_provider = Some(provider);
        self
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

    /// Deduplicates entities by delegating to
    /// [`KnowledgeStore::run_entity_dedup_with_embeddings`].
    ///
    /// When an [`EmbeddingProvider`] is attached and is not a degraded
    /// sentinel, this backfills any NULL `entities.name_embedding`s
    /// before scoring. That is the wire that makes the
    /// `MergeDecision::AutoMerge` threshold (≥ 0.90) reachable from this
    /// scheduled task — without embeddings the maximum composite score
    /// is 0.70 (#4165 Path A).
    fn deduplicate_entities(&self, nous_id: &str) -> oikonomos::error::Result<MaintenanceReport> {
        let provider_ref = self
            .embedding_provider
            .as_deref()
            .filter(|p| !is_degraded_provider(*p));
        let records = self
            .store
            .run_entity_dedup_with_embeddings_and_tuning(nous_id, provider_ref, &self.tuning)
            .map_err(|e| {
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

        // WHY (#5531): check staleness before recomputing — skip the expensive
        // PageRank+Louvain recomputation when scores are fresh (< 6h old).
        let graph_staleness_threshold = jiff::SignedDuration::from_secs(6 * 60 * 60);
        let should_recompute = self
            .store
            .load_graph_context()
            .map_or(true, |ctx| ctx.is_stale(graph_staleness_threshold)); // WHY: fail-open — if we can't load context, recompute

        if !should_recompute {
            let duration_ms = start.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
            let detail = "Graph recompute: scores are fresh, skipping recomputation".to_owned();
            tracing::debug!(%detail, duration_ms, "maintenance: graph recompute skipped (fresh)");
            return Ok(MaintenanceReport {
                items_processed: 0,
                items_modified: 0,
                errors: 0,
                duration_ms,
                detail: Some(detail),
            });
        }

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
        let start = std::time::Instant::now();

        // WHY: reuse the same workspace-root resolution as the `code_graph_query`
        // MCP tool so manual and scheduled rebuilds behave identically.
        let workspace_root = std::env::var("GNOSIS_WORKSPACE_ROOT").map_or_else(
            |_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            PathBuf::from,
        );

        let graph = gnosis::CodeGraph::open_default(&workspace_root).map_err(|e| {
            oikonomos::error::TaskFailedSnafu {
                task_id: "index-maintenance".to_owned(),
                reason: format!("failed to open gnosis index: {e}"),
            }
            .build()
        })?;

        graph.rebuild().map_err(|e| {
            oikonomos::error::TaskFailedSnafu {
                task_id: "index-maintenance".to_owned(),
                reason: format!("gnosis rebuild failed: {e}"),
            }
            .build()
        })?;

        let duration_ms = start.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
        let detail = format!(
            "gnosis index rebuilt for workspace {}",
            workspace_root.display()
        );

        Ok(MaintenanceReport {
            items_processed: 0,
            items_modified: 0,
            duration_ms,
            detail: Some(detail),
            ..Default::default()
        })
    }

    fn health_check(&self, _nous_id: &str) -> oikonomos::error::Result<MaintenanceReport> {
        Err(oikonomos::error::TaskFailedSnafu {
            task_id: "graph-health-check".to_owned(),
            reason: "knowledge graph health check has no concrete diagnostic contract and is not scheduled"
                .to_owned(),
        }
        .build())
    }

    fn materialize_derived_facts(&self) -> oikonomos::error::Result<MaintenanceReport> {
        let start = std::time::Instant::now();
        let count = self.store.materialize_derived_facts().map_err(|e| {
            oikonomos::error::TaskFailedSnafu {
                task_id: "derived-facts-materialize".to_owned(),
                reason: e.to_string(),
            }
            .build()
        })?;

        let duration_ms = start.elapsed().as_millis().try_into().unwrap_or(u64::MAX);

        #[expect(
            clippy::as_conversions,
            reason = "usize→u64: derived fact count fits in u64"
        )]
        let count_u64 = count as u64;
        let detail = format!("Derived facts materialized: {count_u64}");
        tracing::info!(%detail, duration_ms, "maintenance: derived facts materialization complete");

        Ok(MaintenanceReport {
            items_processed: count_u64,
            items_modified: count_u64,
            duration_ms,
            detail: Some(detail),
            ..Default::default()
        })
    }

    fn discover_serendipitous_facts(
        &self,
        nous_id: &str,
    ) -> oikonomos::error::Result<MaintenanceReport> {
        let start = std::time::Instant::now();
        let report = self
            .store
            .discover_serendipitous_facts(nous_id)
            .map_err(|e| {
                oikonomos::error::TaskFailedSnafu {
                    task_id: "serendipity-discovery".to_owned(),
                    reason: e.to_string(),
                }
                .build()
            })?;

        let duration_ms = start.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
        let items_processed = report.items_processed;
        let items_modified = report.items_modified;
        let discovery_count = report.discovery_count;
        let detail = report.detail.unwrap_or_else(|| {
            format!("Serendipity discovery: {discovery_count} surfaced discoveries")
        });
        tracing::info!(%detail, duration_ms, "maintenance: serendipity discovery complete");

        Ok(MaintenanceReport {
            items_processed,
            items_modified,
            duration_ms,
            detail: Some(detail),
            ..Default::default()
        })
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

    fn consolidate_knowledge(&self, nous_id: &str) -> oikonomos::error::Result<MaintenanceReport> {
        let Some(provider) = self.consolidation_provider.as_ref() else {
            let detail = "Knowledge consolidation skipped: no consolidation provider configured";
            tracing::debug!(%detail, "maintenance: consolidation skipped");
            return Ok(MaintenanceReport {
                detail: Some(detail.to_owned()),
                ..Default::default()
            });
        };

        let results = self
            .store
            .consolidate_knowledge(
                provider.as_ref(),
                nous_id,
                &ConsolidationConfig::default(),
                false,
            )
            .map_err(|e| {
                oikonomos::error::TaskFailedSnafu {
                    task_id: "knowledge-consolidation".to_owned(),
                    reason: e.to_string(),
                }
                .build()
            })?;

        #[expect(
            clippy::as_conversions,
            reason = "fact counts are bounded by batch limits"
        )]
        let items_processed: u64 = results.iter().map(|r| r.original_count as u64).sum();
        #[expect(
            clippy::as_conversions,
            reason = "fact counts are bounded by batch limits"
        )]
        let items_modified: u64 = results.iter().map(|r| r.consolidated_count as u64).sum();
        let candidate_count = results.len();

        let detail = format!(
            "Knowledge consolidation: {items_processed} facts examined, {items_modified} consolidated facts produced across {candidate_count} candidates"
        );
        tracing::info!(%detail, "maintenance: knowledge consolidation complete");

        Ok(MaintenanceReport {
            items_processed,
            items_modified,
            detail: Some(detail),
            ..Default::default()
        })
    }
}

/// Bridges a [`ProviderRegistry`] to the synchronous
/// [`ConsolidationProvider`] trait used by the episteme engine.
///
/// WHY (#5530): `consolidate_knowledge` is synchronous because it runs on
/// the daemon's blocking thread pool. The configured LLM provider is
/// asynchronous, so this wrapper uses the current Tokio runtime handle to
/// drive the completion and extracts the raw text response.
pub(crate) struct LlmConsolidationProvider {
    registry: Arc<ProviderRegistry>,
    model: String,
}

impl LlmConsolidationProvider {
    /// Create a provider that resolves the configured model from the
    /// registry on each consolidation call.
    pub(crate) fn new(registry: Arc<ProviderRegistry>, model: String) -> Self {
        Self { registry, model }
    }
}

impl ConsolidationProvider for LlmConsolidationProvider {
    fn consolidate(
        &self,
        system: &str,
        user_message: &str,
    ) -> Result<String, episteme::consolidation::ConsolidationError> {
        let provider = self.registry.find_provider(&self.model).ok_or_else(|| {
            episteme::consolidation::ConsolidationError::LlmCall {
                message: format!("no LLM provider found for model {}", self.model),
                location: snafu::location!(),
            }
        })?;

        let request = CompletionRequest {
            model: self.model.clone(),
            system: Some(system.to_owned()),
            messages: vec![Message {
                role: Role::User,
                content: Content::Text(user_message.to_owned()),
                cache_breakpoint: false,
            }],
            max_tokens: 4096,
            temperature: Some(0.0),
            ..CompletionRequest::default()
        };

        let runtime = tokio::runtime::Handle::try_current().map_err(|e| {
            episteme::consolidation::ConsolidationError::LlmCall {
                message: format!("no Tokio runtime available for consolidation LLM call: {e}"),
                location: snafu::location!(),
            }
        })?;

        let response = runtime.block_on(provider.complete(&request)).map_err(|e| {
            episteme::consolidation::ConsolidationError::LlmCall {
                message: format!("consolidation LLM call failed: {e}"),
                location: snafu::location!(),
            }
        })?;

        let text = response
            .content
            .iter()
            .filter_map(ContentBlock::text)
            .collect::<Vec<_>>()
            .join("\n");

        Ok(text)
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::collections::BTreeMap;

    use mneme::engine::DataValue;
    use mneme::id::{EntityId, FactId};
    use mneme::knowledge::{
        EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactSensitivity,
        FactTemporal, Visibility, far_future,
    };

    use super::*;

    fn make_fact(
        id: &str,
        nous_id: &str,
        content: &str,
        access_count: u32,
        last_accessed_at: Option<jiff::Timestamp>,
    ) -> Fact {
        let recorded_at = jiff::Timestamp::now();
        Fact {
            id: FactId::new(id).expect("valid test id"),
            nous_id: nous_id.to_owned(),
            fact_type: "observation".to_owned(),
            content: content.to_owned(),
            scope: None,
            project_id: None,
            sensitivity: FactSensitivity::Public,
            visibility: Visibility::Private,
            temporal: FactTemporal {
                valid_from: recorded_at,
                valid_to: far_future(),
                recorded_at,
            },
            provenance: FactProvenance {
                confidence: 0.9,
                tier: EpistemicTier::Verified,
                source_session_id: Some("seed-session".to_owned()),
                stability_hours: 720.0,
            },
            lifecycle: FactLifecycle {
                superseded_by: None,
                is_forgotten: false,
                forgotten_at: None,
                forget_reason: None,
            },
            access: FactAccess {
                access_count,
                last_accessed_at,
            },
        }
    }

    fn make_fact_with_recorded_at(
        id: &str,
        nous_id: &str,
        content: &str,
        access_count: u32,
        last_accessed_at: Option<jiff::Timestamp>,
        recorded_at: jiff::Timestamp,
    ) -> Fact {
        Fact {
            id: FactId::new(id).expect("valid test id"),
            nous_id: nous_id.to_owned(),
            fact_type: "observation".to_owned(),
            content: content.to_owned(),
            scope: None,
            project_id: None,
            sensitivity: FactSensitivity::Public,
            visibility: Visibility::Private,
            temporal: FactTemporal {
                valid_from: recorded_at,
                valid_to: far_future(),
                recorded_at,
            },
            provenance: FactProvenance {
                confidence: 0.9,
                tier: EpistemicTier::Inferred,
                source_session_id: Some("seed-session".to_owned()),
                stability_hours: 720.0,
            },
            lifecycle: FactLifecycle {
                superseded_by: None,
                is_forgotten: false,
                forgotten_at: None,
                forget_reason: None,
            },
            access: FactAccess {
                access_count,
                last_accessed_at,
            },
        }
    }

    fn link_fact_entity(store: &Arc<KnowledgeStore>, fact_id: &FactId, entity_id: &EntityId) {
        let mut params = BTreeMap::new();
        params.insert(
            "fact_id".to_owned(),
            DataValue::Str(fact_id.as_str().to_owned().into()),
        );
        params.insert(
            "entity_id".to_owned(),
            DataValue::Str(entity_id.as_str().to_owned().into()),
        );
        params.insert(
            "created_at".to_owned(),
            DataValue::Str(mneme::knowledge::format_timestamp(&jiff::Timestamp::now()).into()),
        );
        store
            .run_mut_query(
                "?[fact_id, entity_id, created_at] <- [[$fact_id, $entity_id, $created_at]]\n:put fact_entities {fact_id, entity_id => created_at}",
                params,
            )
            .expect("link fact to entity");
    }

    #[expect(
        clippy::too_many_lines,
        reason = "seeded maintenance integration covers the full flow"
    )]
    #[test]
    fn discover_serendipitous_facts_runs_on_seeded_store() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = KnowledgeStore::open_fjall(
            dir.path().join("knowledge"),
            mneme::knowledge_store::KnowledgeConfig::default(),
        )
        .expect("open store");

        let alice = EntityId::new("alice").expect("valid entity id");
        let bob = EntityId::new("bob").expect("valid entity id");
        let acme = EntityId::new("acme.corp").expect("valid entity id");

        for (entity_id, name, entity_type) in [
            (&alice, "Alice", "person"),
            (&bob, "Bob", "person"),
            (&acme, "Acme Corp", "company"),
        ] {
            store
                .insert_entity(&mneme::knowledge::Entity {
                    id: entity_id.clone(),
                    name: name.to_owned(),
                    entity_type: entity_type.to_owned(),
                    aliases: Vec::new(),
                    created_at: jiff::Timestamp::now(),
                    updated_at: jiff::Timestamp::now(),
                })
                .expect("insert entity");
        }

        for (src, dst, relation) in [
            (&alice, &bob, "collaborates_with"),
            (&bob, &acme, "documents_for"),
            (&acme, &alice, "contracts_with"),
        ] {
            store
                .insert_relationship(&mneme::knowledge::Relationship {
                    src: src.clone(),
                    dst: dst.clone(),
                    relation: relation.to_owned(),
                    weight: 0.8,
                    created_at: jiff::Timestamp::now(),
                })
                .expect("insert relationship");
        }

        let now = jiff::Timestamp::now();
        let one_hour_ago = now
            .checked_sub(jiff::SignedDuration::from_hours(1))
            .expect("timestamp arithmetic");
        let twelve_hours_ago = now
            .checked_sub(jiff::SignedDuration::from_hours(12))
            .expect("timestamp arithmetic");
        let forty_eight_hours_ago = now
            .checked_sub(jiff::SignedDuration::from_hours(48))
            .expect("timestamp arithmetic");

        let facts = [
            (
                make_fact(
                    "fact-alice",
                    "alice",
                    "Alice keeps the ops feed tidy.",
                    3,
                    Some(one_hour_ago),
                ),
                &alice,
            ),
            (
                make_fact(
                    "fact-bob",
                    "alice",
                    "Bob is the bridge between ops and archives.",
                    2,
                    Some(twelve_hours_ago),
                ),
                &bob,
            ),
            (
                make_fact(
                    "fact-acme",
                    "alice",
                    "Acme Corp owns the incident response handbook.",
                    1,
                    Some(forty_eight_hours_ago),
                ),
                &acme,
            ),
        ];

        for (fact, entity) in facts {
            store.insert_fact(&fact).expect("insert fact");
            link_fact_entity(&store, &fact.id, entity);
        }

        store
            .recompute_graph_scores()
            .expect("recompute graph scores");

        let adapter = KnowledgeMaintenanceAdapter::new(Arc::clone(&store));
        let report = adapter
            .discover_serendipitous_facts("alice")
            .expect("discover serendipity");

        assert!(report.items_processed >= 3);
        assert!(
            report
                .detail
                .as_deref()
                .is_some_and(|detail| detail.contains("Serendipity discovery")),
            "detail should summarize the discovery pass"
        );
    }

    /// Mock provider that returns a fixed consolidation JSON response.
    struct MockConsolidationProvider {
        response: String,
    }

    impl ConsolidationProvider for MockConsolidationProvider {
        fn consolidate(
            &self,
            _system: &str,
            _user_message: &str,
        ) -> Result<String, episteme::consolidation::ConsolidationError> {
            Ok(self.response.clone())
        }
    }

    #[test]
    fn consolidate_knowledge_runs_on_overflowing_entity() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = KnowledgeStore::open_fjall(
            dir.path().join("knowledge"),
            mneme::knowledge_store::KnowledgeConfig::default(),
        )
        .expect("open store");

        let alice = EntityId::new("alice").expect("valid entity id");
        store
            .insert_entity(&mneme::knowledge::Entity {
                id: alice.clone(),
                name: "Alice".to_owned(),
                entity_type: "person".to_owned(),
                aliases: Vec::new(),
                created_at: jiff::Timestamp::now(),
                updated_at: jiff::Timestamp::now(),
            })
            .expect("insert entity");

        let recorded_at = jiff::Timestamp::now()
            .checked_sub(jiff::SignedDuration::from_secs(10 * 24 * 60 * 60))
            .expect("timestamp arithmetic");

        for i in 0..12 {
            let fact = make_fact_with_recorded_at(
                &format!("fact-{i}"),
                "alice",
                &format!("Alice observation {i}"),
                1,
                None,
                recorded_at,
            );
            store.insert_fact(&fact).expect("insert fact");
            link_fact_entity(&store, &fact.id, &alice);
        }

        let provider = Arc::new(MockConsolidationProvider {
            response: r#"[{"content": "Alice has a dozen observations."}]"#.to_owned(),
        });
        let adapter = KnowledgeMaintenanceAdapter::new(Arc::clone(&store))
            .with_consolidation_provider(provider);

        let report = adapter
            .consolidate_knowledge("alice")
            .expect("consolidate knowledge");

        assert!(
            report.items_processed >= 10,
            "should process the overflowing entity's facts"
        );
        assert!(
            report.items_modified >= 1,
            "should produce at least one consolidated fact"
        );
        assert!(
            report
                .detail
                .as_deref()
                .is_some_and(|detail| detail.contains("Knowledge consolidation")),
            "detail should summarize the consolidation pass"
        );
    }
}
