//! Adapter bridging `KnowledgeStore` + `EmbeddingProvider` to `KnowledgeSearchService`.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use aletheia_mneme::embedding::EmbeddingProvider;
use aletheia_mneme::knowledge::{
    EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactTemporal,
};
use aletheia_mneme::knowledge_store::{HybridQuery, KnowledgeStore};
use aletheia_organon::error::{
    DatalogQuerySnafu, EmbeddingSnafu, FactQuerySnafu, InvalidReasonSnafu, KnowledgeAdapterError,
    MutateStoreSnafu, SearchSnafu,
};
use aletheia_organon::types::{DatalogResult, FactSummary, KnowledgeSearchService, MemoryResult};

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
            let embedding = self.embedder.embed(&query).map_err(|e| {
                EmbeddingSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;

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
                .map_err(|e| {
                    SearchSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;

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
                .map_err(|e| {
                    MutateStoreSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;

            let ts_now = jiff::Timestamp::now();
            let new_fact = Fact {
                id: aletheia_mneme::id::FactId::new(new_id.as_str()).map_err(|e| {
                    MutateStoreSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?,
                nous_id,
                fact_type: String::new(),
                content: new_content,
                temporal: FactTemporal {
                    valid_from: ts_now,
                    valid_to: aletheia_mneme::knowledge::far_future(),
                    recorded_at: ts_now,
                },
                provenance: FactProvenance {
                    confidence: 1.0,
                    tier: EpistemicTier::Verified,
                    source_session_id: None,
                    stability_hours: aletheia_mneme::knowledge::default_stability_hours(""),
                },
                lifecycle: FactLifecycle {
                    superseded_by: None,
                    is_forgotten: false,
                    forgotten_at: None,
                    forget_reason: None,
                },
                access: FactAccess {
                    access_count: 0,
                    last_accessed_at: None,
                },
            };
            self.store.insert_fact(&new_fact).map_err(|e| {
                MutateStoreSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;

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
            self.store.run_mut_query(script, params).map_err(|e| {
                MutateStoreSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
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
                .map_err(|e| {
                    FactQuerySnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;

            let since_ts = since
                .as_deref()
                .and_then(aletheia_mneme::knowledge::parse_timestamp)
                .unwrap_or(jiff::Timestamp::UNIX_EPOCH);
            let out = facts
                .into_iter()
                .filter(|f| f.temporal.recorded_at >= since_ts)
                .map(|f| FactSummary {
                    id: f.id.to_string(),
                    content: f.content,
                    confidence: f.provenance.confidence,
                    tier: f.provenance.tier.to_string(),
                    recorded_at: aletheia_mneme::knowledge::format_timestamp(
                        &f.temporal.recorded_at,
                    ),
                    is_forgotten: f.lifecycle.is_forgotten,
                    forgotten_at: f.lifecycle.forgotten_at.map(|s| s.to_string()),
                    forget_reason: f.lifecycle.forget_reason.map(|r| r.to_string()),
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
        let fact_id_str = fact_id.to_owned();
        let reason = reason.to_owned();
        Box::pin(async move {
            let fact_id = aletheia_mneme::id::FactId::new(fact_id_str).map_err(|e| {
                MutateStoreSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
            let reason: aletheia_mneme::knowledge::ForgetReason = reason
                .parse()
                .map_err(|e: String| InvalidReasonSnafu { reason: e }.build())?;
            let fact = self
                .store
                .forget_fact_async(fact_id, reason)
                .await
                .map_err(|e| {
                    MutateStoreSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;
            Ok(fact_to_summary(fact))
        })
    }

    fn unforget_fact(
        &self,
        fact_id: &str,
    ) -> Pin<Box<dyn Future<Output = Result<FactSummary, KnowledgeAdapterError>> + Send + '_>> {
        let fact_id_str = fact_id.to_owned();
        Box::pin(async move {
            let fact_id = aletheia_mneme::id::FactId::new(fact_id_str).map_err(|e| {
                MutateStoreSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
            let fact = self.store.unforget_fact_async(fact_id).await.map_err(|e| {
                MutateStoreSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
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
                .map_err(|e| {
                    DatalogQuerySnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;

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
        confidence: f.provenance.confidence,
        tier: f.provenance.tier.to_string(),
        recorded_at: aletheia_mneme::knowledge::format_timestamp(&f.temporal.recorded_at),
        is_forgotten: f.lifecycle.is_forgotten,
        forgotten_at: f.lifecycle.forgotten_at.map(|t| t.to_string()),
        forget_reason: f.lifecycle.forget_reason.map(|r| r.to_string()),
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
