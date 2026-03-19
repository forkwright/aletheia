//! Adapter bridging `KnowledgeStore` + `EmbeddingProvider` to `KnowledgeSearchService`.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use snafu::ResultExt;

use aletheia_mneme::embedding::EmbeddingProvider;
use aletheia_mneme::knowledge::{EpistemicTier, Fact};
use aletheia_mneme::knowledge_store::{HybridQuery, KnowledgeStore};
use aletheia_organon::error::{
    DatalogQuerySnafu, EmbeddingSnafu, FactQuerySnafu, InvalidReasonSnafu, KnowledgeAdapterError,
    MutateStoreSnafu, SearchSnafu,
};
use aletheia_organon::types::{DatalogResult, FactSummary, KnowledgeSearchService, MemoryResult};

/// Boxes any error type for use with snafu context selectors that expect
/// `Box<dyn Error + Send + Sync>` as source.
trait BoxErr<T> {
    fn box_err(self) -> Result<T, Box<dyn std::error::Error + Send + Sync>>;
}

impl<T, E: std::error::Error + Send + Sync + 'static> BoxErr<T> for Result<T, E> {
    fn box_err(self) -> Result<T, Box<dyn std::error::Error + Send + Sync>> {
        #[expect(
            clippy::as_conversions,
            reason = "coercion to Box<dyn Error + Send + Sync> trait object"
        )]
        self.map_err(|e| Box::new(e) as _)
    }
}

pub(crate) struct KnowledgeSearchAdapter {
    store: Arc<KnowledgeStore>,
    embedder: Arc<dyn EmbeddingProvider>,
}

impl KnowledgeSearchAdapter {
    pub(crate) fn new(store: Arc<KnowledgeStore>, embedder: Arc<dyn EmbeddingProvider>) -> Self {
        Self { store, embedder }
    }
}

impl KnowledgeSearchService for KnowledgeSearchAdapter {
    fn search(
        &self,
        query: &str,
        nous_id: &str,
        limit: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<MemoryResult>, KnowledgeAdapterError>> + Send + '_>>
    {
        let query = query.to_owned();
        let nous_id = nous_id.to_owned();
        Box::pin(async move {
            let embedding = self
                .embedder
                .embed(&query)
                .box_err()
                .context(EmbeddingSnafu)?;

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
                .box_err()
                .context(SearchSnafu)?;

            let now = jiff::Zoned::now()
                .strftime("%Y-%m-%dT%H:%M:%SZ")
                .to_string();
            let facts = self
                .store
                .query_facts_async(nous_id, now, i64::try_from(limit).unwrap_or(i64::MAX))
                .await
                .unwrap_or_else(|e| {
                    tracing::warn!(error = %e, "fact query failed, returning empty results");
                    Vec::new()
                });
            let fact_map: std::collections::HashMap<&str, &Fact> =
                facts.iter().map(|f| (f.id.as_str(), f)).collect();

            let mut out = Vec::with_capacity(results.len());
            for r in results {
                let content = match fact_map.get(r.id.as_str()) {
                    Some(f) => f.content.clone(),
                    None => format!("[fact {}]", r.id),
                };
                out.push(MemoryResult {
                    id: r.id.to_string(),
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
    ) -> Pin<Box<dyn Future<Output = Result<String, KnowledgeAdapterError>> + Send + '_>> {
        let fact_id = fact_id.to_owned();
        let new_content = new_content.to_owned();
        let nous_id = nous_id.to_owned();
        Box::pin(async move {
            let now = jiff::Zoned::now()
                .strftime("%Y-%m-%dT%H:%M:%SZ")
                .to_string();
            let new_id = format!("fact-{}", ulid::Ulid::new());

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
                .box_err()
                .context(MutateStoreSnafu)?;

            let ts_now = jiff::Timestamp::now();
            let new_fact = Fact {
                id: aletheia_mneme::id::FactId::from(new_id.as_str()),
                nous_id,
                content: new_content,
                confidence: 1.0,
                tier: EpistemicTier::Verified,
                valid_from: ts_now,
                valid_to: aletheia_mneme::knowledge::far_future(),
                superseded_by: None,
                source_session_id: None,
                recorded_at: ts_now,
                access_count: 0,
                last_accessed_at: None,
                stability_hours: aletheia_mneme::knowledge::default_stability_hours(""),
                fact_type: String::new(),
                is_forgotten: false,
                forgotten_at: None,
                forget_reason: None,
            };
            self.store
                .insert_fact(&new_fact)
                .box_err()
                .context(MutateStoreSnafu)?;

            Ok(new_id)
        })
    }

    fn retract_fact(
        &self,
        fact_id: &str,
        _reason: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<(), KnowledgeAdapterError>> + Send + '_>> {
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
                .box_err()
                .context(MutateStoreSnafu)?;
            Ok(())
        })
    }

    fn audit_facts(
        &self,
        nous_id: Option<&str>,
        since: Option<&str>,
        limit: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<FactSummary>, KnowledgeAdapterError>> + Send + '_>>
    {
        let nous_id = nous_id.map(str::to_owned);
        let since = since.map(str::to_owned);
        Box::pin(async move {
            let agent = nous_id.as_deref().unwrap_or("");
            let facts = self
                .store
                .audit_all_facts_async(agent.to_owned(), i64::try_from(limit).unwrap_or(i64::MAX))
                .await
                .box_err()
                .context(FactQuerySnafu)?;

            let since_ts = since
                .as_deref()
                .and_then(aletheia_mneme::knowledge::parse_timestamp)
                .unwrap_or(jiff::Timestamp::UNIX_EPOCH);
            let out = facts
                .into_iter()
                .filter(|f| f.recorded_at >= since_ts)
                .map(|f| FactSummary {
                    id: f.id.to_string(),
                    content: f.content,
                    confidence: f.confidence,
                    tier: f.tier.to_string(),
                    recorded_at: aletheia_mneme::knowledge::format_timestamp(&f.recorded_at),
                    is_forgotten: f.is_forgotten,
                    forgotten_at: f.forgotten_at.map(|s| s.to_string()),
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
    ) -> Pin<Box<dyn Future<Output = Result<FactSummary, KnowledgeAdapterError>> + Send + '_>> {
        let fact_id = aletheia_mneme::id::FactId::from(fact_id);
        let reason = reason.to_owned();
        Box::pin(async move {
            let reason: aletheia_mneme::knowledge::ForgetReason = reason
                .parse()
                .map_err(|e: String| InvalidReasonSnafu { reason: e }.build())?;
            let fact = self
                .store
                .forget_fact_async(fact_id, reason)
                .await
                .box_err()
                .context(MutateStoreSnafu)?;
            Ok(fact_to_summary(fact))
        })
    }

    fn unforget_fact(
        &self,
        fact_id: &str,
    ) -> Pin<Box<dyn Future<Output = Result<FactSummary, KnowledgeAdapterError>> + Send + '_>> {
        let fact_id = aletheia_mneme::id::FactId::from(fact_id);
        Box::pin(async move {
            let fact = self
                .store
                .unforget_fact_async(fact_id)
                .await
                .box_err()
                .context(MutateStoreSnafu)?;
            Ok(fact_to_summary(fact))
        })
    }

    fn datalog_query(
        &self,
        query: &str,
        params: Option<serde_json::Value>,
        timeout_secs: Option<f64>,
        row_limit: Option<usize>,
    ) -> Pin<Box<dyn Future<Output = Result<DatalogResult, KnowledgeAdapterError>> + Send + '_>>
    {
        let query = query.to_owned();
        let row_limit = row_limit.unwrap_or(100);
        let timeout = timeout_secs.map(std::time::Duration::from_secs_f64);
        Box::pin(async move {
            let mut cozo_params = std::collections::BTreeMap::new();
            if let Some(serde_json::Value::Object(map)) = params {
                for (k, v) in map {
                    let dv = json_to_datavalue(&v);
                    cozo_params.insert(k, dv);
                }
            }

            let rows = self
                .store
                .run_query_with_timeout(&query, cozo_params, timeout)
                .box_err()
                .context(DatalogQuerySnafu)?;

            let columns = rows.headers.iter().map(ToString::to_string).collect();
            let truncated = rows.rows.len() > row_limit;
            let result_rows: Vec<Vec<serde_json::Value>> = rows
                .rows
                .into_iter()
                .take(row_limit)
                .map(|row| row.iter().map(datavalue_to_json).collect())
                .collect();

            Ok(DatalogResult {
                columns,
                rows: result_rows,
                truncated,
            })
        })
    }
}

fn fact_to_summary(f: Fact) -> FactSummary {
    FactSummary {
        id: f.id.to_string(),
        content: f.content,
        confidence: f.confidence,
        tier: f.tier.to_string(),
        recorded_at: aletheia_mneme::knowledge::format_timestamp(&f.recorded_at),
        is_forgotten: f.is_forgotten,
        forgotten_at: f.forgotten_at.map(|t| t.to_string()),
        forget_reason: f.forget_reason.map(|r| r.to_string()),
    }
}

fn json_to_datavalue(v: &serde_json::Value) -> aletheia_mneme::engine::DataValue {
    use aletheia_mneme::engine::DataValue;
    match v {
        serde_json::Value::Null => DataValue::Null,
        serde_json::Value::Bool(b) => DataValue::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                DataValue::from(i)
            } else if let Some(f) = n.as_f64() {
                DataValue::from(f)
            } else {
                DataValue::Null
            }
        }
        serde_json::Value::String(s) => DataValue::Str(s.as_str().into()),
        _ => DataValue::Str(v.to_string().into()),
    }
}

fn datavalue_to_json(v: &aletheia_mneme::engine::DataValue) -> serde_json::Value {
    use aletheia_mneme::engine::DataValue;
    match v {
        DataValue::Null => serde_json::Value::Null,
        DataValue::Bool(b) => serde_json::Value::Bool(*b),
        DataValue::Str(s) => serde_json::Value::String(s.to_string()),
        dv => {
            if let Some(i) = dv.get_int() {
                serde_json::Value::Number(serde_json::Number::from(i))
            } else if let Some(f) = dv.get_float() {
                serde_json::Number::from_f64(f)
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::String(f.to_string()))
            } else {
                serde_json::Value::String(format!("{dv:?}"))
            }
        }
    }
}
