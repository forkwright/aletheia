//! Adapter bridging `KnowledgeStore` + `EmbeddingProvider` to `KnowledgeSearchService`.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use aletheia_mneme::embedding::EmbeddingProvider;
use aletheia_mneme::knowledge::{EpistemicTier, Fact};
use aletheia_mneme::knowledge_store::{HybridQuery, KnowledgeStore};
use aletheia_organon::types::{FactSummary, KnowledgeSearchService, MemoryResult};

pub struct KnowledgeSearchAdapter {
    store: Arc<KnowledgeStore>,
    embedder: Arc<dyn EmbeddingProvider>,
}

impl KnowledgeSearchAdapter {
    pub fn new(store: Arc<KnowledgeStore>, embedder: Arc<dyn EmbeddingProvider>) -> Self {
        Self { store, embedder }
    }
}

impl KnowledgeSearchService for KnowledgeSearchAdapter {
    fn search(
        &self,
        query: &str,
        nous_id: &str,
        limit: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<MemoryResult>, String>> + Send + '_>> {
        let query = query.to_owned();
        let nous_id = nous_id.to_owned();
        Box::pin(async move {
            let embedding = self
                .embedder
                .embed(&query)
                .map_err(|e| format!("embedding failed: {e}"))?;

            let hybrid_query = HybridQuery {
                text: query,
                embedding,
                seed_entities: vec![],
                limit,
                ef: 200,
            };

            let results = self
                .store
                .search_hybrid_async(hybrid_query)
                .await
                .map_err(|e| format!("hybrid search failed: {e}"))?;

            // Resolve fact content from IDs via a point-in-time query
            let now = jiff::Zoned::now()
                .strftime("%Y-%m-%dT%H:%M:%SZ")
                .to_string();
            let facts = self
                .store
                .query_facts_async(nous_id, now, i64::try_from(limit).unwrap_or(i64::MAX))
                .await
                .unwrap_or_default();
            let fact_map: std::collections::HashMap<&str, &Fact> =
                facts.iter().map(|f| (f.id.as_str(), f)).collect();

            let mut out = Vec::with_capacity(results.len());
            for r in results {
                let content = match fact_map.get(r.id.as_str()) {
                    Some(f) => f.content.clone(),
                    None => format!("[fact {}]", r.id),
                };
                out.push(MemoryResult {
                    id: r.id,
                    content,
                    score: r.rrf_score,
                    source_type: "fact".to_owned(),
                });
            }
            Ok(out)
        })
    }

    fn correct_fact(
        &self,
        fact_id: &str,
        new_content: &str,
        nous_id: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>> {
        let fact_id = fact_id.to_owned();
        let new_content = new_content.to_owned();
        let nous_id = nous_id.to_owned();
        Box::pin(async move {
            let now = jiff::Zoned::now()
                .strftime("%Y-%m-%dT%H:%M:%SZ")
                .to_string();
            let new_id = format!("fact-{}", ulid::Ulid::new());

            // Mark old fact as superseded
            let retract_script = r"
                ?[id, valid_from, content, nous_id, confidence, tier, valid_to, superseded_by, source_session_id, recorded_at,
                  access_count, last_accessed_at, stability_hours, fact_type,
                  is_forgotten, forgotten_at, forget_reason] :=
                    *facts{id: $old_id, valid_from, content, nous_id, confidence, tier, source_session_id, recorded_at,
                           access_count, last_accessed_at, stability_hours, fact_type,
                           is_forgotten, forgotten_at, forget_reason},
                    valid_to = $now,
                    superseded_by = $new_id
                :put facts {id, valid_from => content, nous_id, confidence, tier, valid_to, superseded_by, source_session_id, recorded_at,
                            access_count, last_accessed_at, stability_hours, fact_type,
                            is_forgotten, forgotten_at, forget_reason}
            ";
            let mut params = std::collections::BTreeMap::new();
            params.insert(
                "old_id".to_owned(),
                aletheia_mneme::engine::DataValue::Str(fact_id.as_str().into()),
            );
            params.insert(
                "now".to_owned(),
                aletheia_mneme::engine::DataValue::Str(now.as_str().into()),
            );
            params.insert(
                "new_id".to_owned(),
                aletheia_mneme::engine::DataValue::Str(new_id.as_str().into()),
            );
            self.store
                .run_mut_query(retract_script, params)
                .map_err(|e| format!("failed to supersede old fact: {e}"))?;

            // Insert corrected fact
            let new_fact = Fact {
                id: new_id.clone(),
                nous_id,
                content: new_content,
                confidence: 1.0,
                tier: EpistemicTier::Verified,
                valid_from: now.clone(),
                valid_to: "9999-12-31".to_owned(),
                superseded_by: None,
                source_session_id: None,
                recorded_at: now,
                access_count: 0,
                last_accessed_at: String::new(),
                stability_hours: aletheia_mneme::knowledge::default_stability_hours(""),
                fact_type: String::new(),
                is_forgotten: false,
                forgotten_at: None,
                forget_reason: None,
            };
            self.store
                .insert_fact(&new_fact)
                .map_err(|e| format!("failed to insert corrected fact: {e}"))?;

            Ok(new_id)
        })
    }

    fn retract_fact(
        &self,
        fact_id: &str,
        _reason: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>> {
        let fact_id = fact_id.to_owned();
        Box::pin(async move {
            let now = jiff::Zoned::now()
                .strftime("%Y-%m-%dT%H:%M:%SZ")
                .to_string();

            let script = r"
                ?[id, valid_from, content, nous_id, confidence, tier, valid_to, superseded_by, source_session_id, recorded_at,
                  access_count, last_accessed_at, stability_hours, fact_type,
                  is_forgotten, forgotten_at, forget_reason] :=
                    *facts{id: $fact_id, valid_from, content, nous_id, confidence, tier, superseded_by, source_session_id, recorded_at,
                           access_count, last_accessed_at, stability_hours, fact_type,
                           is_forgotten, forgotten_at, forget_reason},
                    valid_to = $now
                :put facts {id, valid_from => content, nous_id, confidence, tier, valid_to, superseded_by, source_session_id, recorded_at,
                            access_count, last_accessed_at, stability_hours, fact_type,
                            is_forgotten, forgotten_at, forget_reason}
            ";
            let mut params = std::collections::BTreeMap::new();
            params.insert(
                "fact_id".to_owned(),
                aletheia_mneme::engine::DataValue::Str(fact_id.as_str().into()),
            );
            params.insert(
                "now".to_owned(),
                aletheia_mneme::engine::DataValue::Str(now.as_str().into()),
            );
            self.store
                .run_mut_query(script, params)
                .map_err(|e| format!("failed to retract fact: {e}"))?;
            Ok(())
        })
    }

    fn audit_facts(
        &self,
        nous_id: Option<&str>,
        since: Option<&str>,
        limit: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<FactSummary>, String>> + Send + '_>> {
        let nous_id = nous_id.map(str::to_owned);
        let since = since.map(str::to_owned);
        Box::pin(async move {
            let agent = nous_id.as_deref().unwrap_or("");
            let facts = self
                .store
                .audit_all_facts_async(agent.to_owned(), i64::try_from(limit).unwrap_or(i64::MAX))
                .await
                .map_err(|e| format!("failed to query facts: {e}"))?;

            let since_filter = since.as_deref().unwrap_or("1970-01-01");
            let out = facts
                .into_iter()
                .filter(|f| f.recorded_at.as_str() >= since_filter)
                .map(|f| FactSummary {
                    id: f.id,
                    content: f.content,
                    confidence: f.confidence,
                    tier: f.tier.to_string(),
                    recorded_at: f.recorded_at,
                    is_forgotten: f.is_forgotten,
                    forgotten_at: f.forgotten_at,
                    forget_reason: f.forget_reason.map(|r| r.to_string()),
                })
                .collect();
            Ok(out)
        })
    }

    fn forget_fact(
        &self,
        fact_id: &str,
        reason: &str,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>> {
        let fact_id = fact_id.to_owned();
        let reason = reason.to_owned();
        Box::pin(async move {
            let reason: aletheia_mneme::knowledge::ForgetReason =
                reason.parse().map_err(|e: String| e)?;
            self.store
                .forget_fact_async(fact_id, reason)
                .await
                .map_err(|e| format!("failed to forget fact: {e}"))
        })
    }

    fn unforget_fact(
        &self,
        fact_id: &str,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>> {
        let fact_id = fact_id.to_owned();
        Box::pin(async move {
            self.store
                .unforget_fact_async(fact_id)
                .await
                .map_err(|e| format!("failed to unforget fact: {e}"))
        })
    }
}
