//! Knowledge search, explain, and timeline handlers.

use axum::Json;
use axum::extract::{Query, State};

use crate::error::{ApiError, BadRequestSnafu};
use crate::extract::Claims;
use crate::state::KnowledgeState;

#[cfg(feature = "knowledge-store")]
use super::SimilarFact;
use super::{
    EntityRelationship, ExplainCandidate, ExplainDecision, ExplainQuery, ExplainResponse,
    FactorScoreBreakdown, FactsQuery, RecallWeightsView, SearchQuery, SearchResponse, SearchResult,
    TimelineEvent, TimelineQuery, TimelineResponse, default_order, default_sort,
};

/// Score a fact stream using the same multi-factor recall engine as the turn
/// recall pipeline and produce a full explanation.
// WHY(#5680): bound the candidate set fetched for scoring/timeline to the requested result count,
// not the whole store, so per-request cost does not scale with total facts ever stored. The
// multiplier keeps enough candidates for meaningful re-ranking; the hard cap bounds worst-case
// allocation. Search/explain use the store's text-recall path before in-memory scoring so the cap
// applies to relevant candidates, not arbitrary native-sort rows. Facts/timeline still use the
// native list/audit path because those endpoints are not relevance searches.
const CANDIDATE_FETCH_MULTIPLIER: usize = 50;
const MAX_CANDIDATE_FETCH: usize = 2_000;

/// Candidate fetch size for a requested result `limit`: `limit * MULTIPLIER`, clamped to
/// `[1, MAX_CANDIDATE_FETCH]`.
fn candidate_fetch_limit(requested: usize) -> usize {
    requested
        .saturating_mul(CANDIDATE_FETCH_MULTIPLIER)
        .clamp(1, MAX_CANDIDATE_FETCH)
}

async fn score_facts_for_query(
    state: &KnowledgeState,
    q: &str,
    policy: &super::KnowledgeReadPolicy<'_>,
    limit: usize,
    now: jiff::Timestamp,
) -> Result<mneme::recall::explain::RecallExplanation, ApiError> {
    let max_search_limit = state.config.read().await.api_limits.max_search_limit;
    if limit > max_search_limit {
        return Err(BadRequestSnafu {
            message: format!("limit must not exceed {max_search_limit}"),
        }
        .build());
    }

    let all_facts =
        get_relevant_stored_facts(state, policy, q, candidate_fetch_limit(limit)).await?;
    let engine = build_recall_engine(state).await;

    Ok(mneme::recall::explain::explain_recall(
        &engine,
        &all_facts,
        q,
        policy.single_target_nous_id(),
        now,
        limit.min(max_search_limit),
    ))
}

async fn get_relevant_stored_facts(
    state: &KnowledgeState,
    policy: &super::KnowledgeReadPolicy<'_>,
    query_text: &str,
    limit: usize,
) -> Result<Vec<mneme::knowledge::Fact>, ApiError> {
    #[cfg(feature = "knowledge-store")]
    if let Some(store) = state.knowledge_store.clone() {
        let fetch_limit = i64::try_from(limit.min(MAX_CANDIDATE_FETCH)).unwrap_or(i64::MAX);
        let query_text = query_text.to_owned();
        let nous_id = policy.single_target_nous_id().map(ToOwned::to_owned);
        let result = tokio::task::spawn_blocking(
            move || -> mneme::knowledge_error::Result<Vec<mneme::knowledge::Fact>> {
                let recall_results = match nous_id.as_deref() {
                    Some(nid) => store.search_text_for_recall_scoped(&query_text, fetch_limit, nid),
                    None => store.search_text_for_recall(&query_text, fetch_limit),
                }?;

                let mut facts = Vec::with_capacity(recall_results.len());
                for recall_result in recall_results {
                    if recall_result.source_type != "fact" {
                        continue;
                    }
                    let hydrated = store.read_facts_by_id(&recall_result.source_id)?;
                    if let Some(fact) = hydrated
                        .into_iter()
                        .find(|fact| !fact.lifecycle.is_forgotten)
                    {
                        facts.push(fact);
                    }
                }
                Ok(facts)
            },
        )
        .await;
        // WHY a query failure is 500 and an absent store is 503: they are different
        // conditions and the sibling `get_entity_relationships` already draws the line
        // here. 503 says the capability is not configured -- a deployment fact, stable
        // until someone changes it. 500 says the store is there and broke, which is a
        // bug report. Collapsing them would leave a caller retrying a 503 forever.
        return match result {
            Ok(Ok(facts)) => Ok(policy.filter_facts(facts)),
            Ok(Err(e)) => Err(ApiError::Internal {
                message: format!("failed to query relevant knowledge facts: {e}"),
                location: snafu::location!(),
            }),
            Err(e) => Err(ApiError::Internal {
                message: format!("knowledge relevance query task panicked: {e}"),
                location: snafu::location!(),
            }),
        };
    }
    #[cfg(not(feature = "knowledge-store"))]
    {
        let _ = (state, policy, query_text, limit);
        std::future::ready(()).await;
    }
    // WHY(#6821) an error rather than an empty Vec: "the store holds nothing" and "there
    // is no store" are different answers, and a caller given `[]` for both cannot tell
    // that its memory is unreachable. It renders as a blank panel either way, which is
    // exactly how an operator concludes the agent has forgotten everything.
    Err(ApiError::ServiceUnavailable {
        message: "knowledge store not enabled on this server".to_owned(),
        location: snafu::location!(),
    })
}

async fn build_recall_engine(state: &KnowledgeState) -> mneme::recall::RecallEngine {
    let config_weights = &state.config.read().await.agents.defaults.recall.weights;
    let weights = mneme::recall::RecallWeights {
        vector_similarity: 0.30,
        decay: config_weights.decay,
        relevance: config_weights.relevance,
        epistemic_tier: config_weights.epistemic_tier,
        relationship_proximity: config_weights.relationship_proximity,
        access_frequency: config_weights.access_frequency,
        graph_importance: config_weights.graph_importance,
        serendipity: 0.0,
        surprise: 0.0,
        evidence_coverage: 0.0,
        convergence: 0.0,
    };
    mneme::recall::RecallEngine::with_weights(weights)
}

/// GET /api/v1/knowledge/search
#[utoipa::path(
    get,
    path = "/api/v1/knowledge/search",
    params(
        ("q" = String, Query, description = "Search query text"),
        ("nous_id" = Option<String>, Query, description = "Filter by agent ID"),
        ("limit" = Option<usize>, Query, description = "Maximum results (default: 20)"),
    ),
    responses(
        (status = 200, description = "Search results ranked by relevance", body = SearchResponse),
        (status = 400, description = "Invalid query or limit", body = crate::error::ErrorResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 500, description = "Knowledge store query failed", body = crate::error::ErrorResponse),
        (status = 503, description = "Knowledge store not enabled on this server", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
pub async fn search(
    State(state): State<KnowledgeState>,
    claims: Claims,
    Query(query): Query<SearchQuery>,
) -> Result<Json<SearchResponse>, ApiError> {
    if query.q.trim().is_empty() {
        return Err(BadRequestSnafu {
            message: "search query 'q' must not be empty",
        }
        .build());
    }

    let policy = super::KnowledgeReadPolicy::from_single_nous(&claims, query.nous_id.as_deref())?;
    let explanation = score_facts_for_query(
        &state,
        &query.q,
        &policy,
        query.limit,
        jiff::Timestamp::now(),
    )
    .await?;

    let results: Vec<SearchResult> = explanation
        .candidates
        .into_iter()
        .filter(|c| c.decision == mneme::recall::explain::CandidateDecision::Selected)
        .map(|c| SearchResult {
            id: c.result.source_id,
            content: c.result.content,
            confidence: c.confidence,
            tier: c.tier,
            fact_type: c.fact_type,
            score: c.result.score,
        })
        .collect();

    Ok(Json(SearchResponse { results }))
}

/// GET /api/v1/knowledge/search/explain
#[utoipa::path(
    get,
    path = "/api/v1/knowledge/search/explain",
    params(
        ("q" = String, Query, description = "Search query text"),
        ("nous_id" = Option<String>, Query, description = "Filter by agent ID"),
        ("limit" = Option<usize>, Query, description = "Maximum results (default: 20)"),
    ),
    responses(
        (status = 200, description = "Explainable recall scoring report", body = ExplainResponse),
        (status = 400, description = "Invalid query or limit", body = crate::error::ErrorResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 500, description = "Knowledge store query failed", body = crate::error::ErrorResponse),
        (status = 503, description = "Knowledge store not enabled on this server", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
pub async fn explain(
    State(state): State<KnowledgeState>,
    claims: Claims,
    Query(query): Query<ExplainQuery>,
) -> Result<Json<ExplainResponse>, ApiError> {
    if query.q.trim().is_empty() {
        return Err(BadRequestSnafu {
            message: "search query 'q' must not be empty",
        }
        .build());
    }

    let policy = super::KnowledgeReadPolicy::from_single_nous(&claims, query.nous_id.as_deref())?;
    let explanation = score_facts_for_query(
        &state,
        &query.q,
        &policy,
        query.limit,
        jiff::Timestamp::now(),
    )
    .await?;

    let mut selected = Vec::new();
    let mut dropped = Vec::new();

    for candidate in explanation.candidates {
        let item = ExplainCandidate {
            id: candidate.result.source_id,
            content: candidate.result.content,
            confidence: candidate.confidence,
            tier: candidate.tier,
            fact_type: candidate.fact_type,
            score: candidate.result.score,
            decision: match candidate.decision {
                mneme::recall::explain::CandidateDecision::Selected => ExplainDecision::Selected,
                mneme::recall::explain::CandidateDecision::Filtered => ExplainDecision::Filtered,
                // Covers Dropped and any future #[non_exhaustive] variants.
                _ => ExplainDecision::Dropped,
            },
            reasons: candidate.reasons,
            factors: FactorScoreBreakdown {
                vector_similarity: candidate.result.factors.vector_similarity,
                decay: candidate.result.factors.decay,
                relevance: candidate.result.factors.relevance,
                epistemic_tier: candidate.result.factors.epistemic_tier,
                access_frequency: candidate.result.factors.access_frequency,
                relationship_proximity: candidate.result.factors.relationship_proximity,
                graph_importance: candidate.result.factors.graph_importance,
            },
        };

        if matches!(item.decision, ExplainDecision::Selected) {
            selected.push(item);
        } else {
            dropped.push(item);
        }
    }

    Ok(Json(ExplainResponse {
        query: query.q,
        weights: RecallWeightsView {
            vector_similarity: explanation.weights.vector_similarity,
            decay: explanation.weights.decay,
            relevance: explanation.weights.relevance,
            epistemic_tier: explanation.weights.epistemic_tier,
            access_frequency: explanation.weights.access_frequency,
            relationship_proximity: explanation.weights.relationship_proximity,
            graph_importance: explanation.weights.graph_importance,
        },
        total_candidates: selected.len() + dropped.len(),
        selected,
        dropped,
    }))
}

/// GET /api/v1/knowledge/timeline
#[utoipa::path(
    get,
    path = "/api/v1/knowledge/timeline",
    params(
        ("nous_id" = Option<String>, Query, description = "Filter by agent ID"),
        ("limit" = Option<usize>, Query, description = "Maximum events (default: 100, max: 1000)"),
        ("offset" = Option<usize>, Query, description = "Pagination offset"),
    ),
    responses(
        (status = 200, description = "Fact activity timeline with total count"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 500, description = "Knowledge store query failed", body = crate::error::ErrorResponse),
        (status = 503, description = "Knowledge store not enabled on this server", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
pub async fn timeline(
    State(state): State<KnowledgeState>,
    claims: Claims,
    Query(mut query): Query<TimelineQuery>,
) -> Result<Json<TimelineResponse>, ApiError> {
    let policy = super::KnowledgeReadPolicy::from_single_nous(&claims, query.nous_id.as_deref())?;
    query.nous_id = policy.single_target_nous_id().map(ToOwned::to_owned);
    let max_facts_limit = state.config.read().await.api_limits.max_facts_limit;
    query.limit = query.limit.min(max_facts_limit);

    let timeline_query = FactsQuery {
        nous_id: query.nous_id,
        sort: default_sort(),
        order: default_order(),
        filter: None,
        fact_type: None,
        tier: None,
        limit: candidate_fetch_limit(query.limit),
        offset: 0,
        include_forgotten: false,
    };
    let facts = get_stored_facts(&state, &policy, &timeline_query).await?;
    let mut events: Vec<TimelineEvent> = Vec::new();

    for fact in &facts {
        if fact.lifecycle.is_forgotten {
            continue;
        }
        events.push(TimelineEvent {
            timestamp: fact.temporal.recorded_at.to_string(),
            event_type: "created".to_string(),
            description: truncate_content(&fact.content, 80),
            fact_id: fact.id.to_string(),
            confidence: Some(fact.provenance.confidence),
        });

        if fact.access.access_count > 0 && fact.access.last_accessed_at.is_some() {
            events.push(TimelineEvent {
                timestamp: fact
                    .access
                    .last_accessed_at
                    .map(|t| t.to_string())
                    .unwrap_or_default(),
                event_type: "accessed".to_string(),
                description: format!(
                    "{} accesses, stability: {:.0}h",
                    fact.access.access_count, fact.provenance.stability_hours
                ),
                fact_id: fact.id.to_string(),
                confidence: Some(fact.provenance.confidence),
            });
        }
    }

    events.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    let total = events.len();

    let start = query.offset.min(events.len());
    let end = (start + query.limit).min(events.len());
    // start and end are both bounded by events.len() via .min()
    #[expect(
        clippy::indexing_slicing,
        reason = "start and end are bounded by events.len() via .min()"
    )]
    let events = events[start..end].to_vec();

    Ok(Json(TimelineResponse { events, total }))
}

pub(super) async fn get_stored_facts(
    state: &KnowledgeState,
    policy: &super::KnowledgeReadPolicy<'_>,
    query: &FactsQuery,
) -> Result<Vec<mneme::knowledge::Fact>, ApiError> {
    #[cfg(feature = "knowledge-store")]
    if let Some(store) = state.knowledge_store.clone() {
        let fetch_limit = i64::try_from((query.offset + query.limit).min(MAX_CANDIDATE_FETCH))
            .unwrap_or(i64::MAX);
        let nous_id = query.nous_id.clone();
        // WHY(#5680): the store list/audit scans are synchronous and grow with the stored fact
        // count; run them on the blocking pool so a large store cannot stall the async runtime
        // under concurrent search load (KnowledgeStore is `Arc`-shared for exactly this).
        let result = tokio::task::spawn_blocking(move || match nous_id.as_deref() {
            Some(nid) => store.audit_all_facts(nid, fetch_limit),
            None => store.list_all_facts(fetch_limit),
        })
        .await;
        return match result {
            Ok(Ok(facts)) => Ok(policy.filter_facts(facts)),
            Ok(Err(e)) => Err(ApiError::Internal {
                message: format!("failed to query knowledge store: {e}"),
                location: snafu::location!(),
            }),
            Err(e) => Err(ApiError::Internal {
                message: format!("knowledge store query task panicked: {e}"),
                location: snafu::location!(),
            }),
        };
    }
    #[cfg(not(feature = "knowledge-store"))]
    {
        let _ = (state, policy, query);
        std::future::ready(()).await;
    }
    // WHY(#6821) an error rather than an empty Vec: "the store holds nothing" and "there
    // is no store" are different answers, and a caller given `[]` for both cannot tell
    // that its memory is unreachable. It renders as a blank panel either way, which is
    // exactly how an operator concludes the agent has forgotten everything.
    Err(ApiError::ServiceUnavailable {
        message: "knowledge store not enabled on this server".to_owned(),
        location: snafu::location!(),
    })
}

pub(super) fn get_stored_entities(
    state: &KnowledgeState,
) -> Result<Vec<mneme::knowledge::Entity>, ApiError> {
    #[cfg(feature = "knowledge-store")]
    if let Some(ref store) = state.knowledge_store {
        return match store.list_entities() {
            Ok(entities) => Ok(entities),
            Err(e) => Err(ApiError::Internal {
                message: format!("failed to query knowledge store entities: {e}"),
                location: snafu::location!(),
            }),
        };
    }
    #[cfg(not(feature = "knowledge-store"))]
    let _ = state;
    // WHY(#6821) an error rather than an empty Vec: "the store holds nothing" and "there
    // is no store" are different answers, and a caller given `[]` for both cannot tell
    // that its memory is unreachable. It renders as a blank panel either way, which is
    // exactly how an operator concludes the agent has forgotten everything.
    Err(ApiError::ServiceUnavailable {
        message: "knowledge store not enabled on this server".to_owned(),
        location: snafu::location!(),
    })
}

#[cfg(feature = "knowledge-store")]
pub(super) fn get_fact_relationships(
    _state: &KnowledgeState,
    _fact: &mneme::knowledge::Fact,
) -> Vec<mneme::knowledge::Relationship> {
    Vec::new()
}

pub(super) fn get_entity_relationships(
    state: &KnowledgeState,
    policy: &super::KnowledgeReadPolicy<'_>,
    entity_id: &str,
) -> Result<Vec<EntityRelationship>, ApiError> {
    #[cfg(feature = "knowledge-store")]
    if let Some(ref store) = state.knowledge_store {
        use std::collections::HashMap;

        let entities = store.list_entities().map_err(|e| ApiError::Internal {
            message: e.to_string(),
            location: snafu::location!(),
        })?;
        let entity_names: HashMap<String, String> = entities
            .into_iter()
            .map(|entity| (entity.id.as_str().to_owned(), entity.name))
            .collect();
        let relationships = store
            .list_all_relationships()
            .map_err(|e| ApiError::Internal {
                message: e.to_string(),
                location: snafu::location!(),
            })?;
        let visible_entities = super::visible_entity_ids(state, policy)?;

        let mut views = Vec::new();
        for relationship in relationships {
            let src = relationship.src.as_str();
            let dst = relationship.dst.as_str();
            let (entity_id, direction) = if src == entity_id {
                (dst.to_owned(), super::RelationshipDirection::Outgoing)
            } else if dst == entity_id {
                (src.to_owned(), super::RelationshipDirection::Incoming)
            } else {
                continue;
            };
            if visible_entities
                .as_ref()
                .is_some_and(|allowed| !allowed.contains(&entity_id))
            {
                continue;
            }
            let entity_name = entity_names
                .get(&entity_id)
                .cloned()
                .unwrap_or_else(|| entity_id.clone());
            views.push(EntityRelationship {
                id: format!(
                    "{src}:{dst}:{}:{}",
                    relationship.relation, relationship.created_at
                ),
                entity_id,
                entity_name,
                relationship_type: relationship.relation,
                direction,
                confidence: relationship.weight,
            });
        }
        return Ok(views);
    }

    #[cfg(not(feature = "knowledge-store"))]
    let _ = (state, policy, entity_id);

    Err(ApiError::ServiceUnavailable {
        message: "knowledge store not enabled on this server".to_owned(),
        location: snafu::location!(),
    })
}

#[cfg(feature = "knowledge-store")]
pub(super) fn get_similar_facts(
    _state: &KnowledgeState,
    _fact: &mneme::knowledge::Fact,
) -> Vec<SimilarFact> {
    Vec::new()
}

pub(super) fn sort_facts(facts: &mut [mneme::knowledge::Fact], sort: &str, order: &str) {
    let desc = order == "desc";
    match sort {
        "confidence" => facts.sort_by(|a, b| {
            let cmp = a
                .provenance
                .confidence
                .partial_cmp(&b.provenance.confidence)
                .unwrap_or(std::cmp::Ordering::Equal);
            if desc { cmp.reverse() } else { cmp }
        }),
        "recency" => facts.sort_by(|a, b| {
            let cmp = a.access.last_accessed_at.cmp(&b.access.last_accessed_at);
            if desc { cmp.reverse() } else { cmp }
        }),
        "created" => facts.sort_by(|a, b| {
            let cmp = a.temporal.recorded_at.cmp(&b.temporal.recorded_at);
            if desc { cmp.reverse() } else { cmp }
        }),
        "access_count" => facts.sort_by(|a, b| {
            let cmp = a.access.access_count.cmp(&b.access.access_count);
            if desc { cmp.reverse() } else { cmp }
        }),
        "fsrs_review" => facts.sort_by(|a, b| {
            let cmp = a
                .provenance
                .stability_hours
                .partial_cmp(&b.provenance.stability_hours)
                .unwrap_or(std::cmp::Ordering::Equal);
            if desc { cmp.reverse() } else { cmp }
        }),
        _ => {
            // NOTE: unrecognized sort field, facts retain original order
        }
    }
}

pub(crate) fn truncate_content(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", s.get(..end).unwrap_or(s))
    }
}

#[cfg(test)]
mod candidate_fetch_tests {
    use super::{CANDIDATE_FETCH_MULTIPLIER, MAX_CANDIDATE_FETCH, candidate_fetch_limit};

    #[test]
    fn fetch_is_proportional_and_bounded() {
        // WHY(#5680): a small requested limit must not fetch the whole store.
        assert_eq!(candidate_fetch_limit(20), 20 * CANDIDATE_FETCH_MULTIPLIER);
        // large requests saturate at the hard cap, never the old unconditional 10k.
        assert_eq!(candidate_fetch_limit(10_000), MAX_CANDIDATE_FETCH);
        assert_eq!(candidate_fetch_limit(1_000), MAX_CANDIDATE_FETCH);
        // never zero (a zero fetch would return no candidates to score).
        assert_eq!(candidate_fetch_limit(0), 1);
        // invariant: fetch is never more than k*N.
        for n in [1usize, 5, 50, 500, 5000] {
            assert!(
                candidate_fetch_limit(n) <= n.saturating_mul(CANDIDATE_FETCH_MULTIPLIER).max(1)
            );
            assert!(candidate_fetch_limit(n) <= MAX_CANDIDATE_FETCH);
        }
    }
}

#[cfg(all(test, feature = "knowledge-store"))]
mod relevance_fetch_tests {
    use std::sync::Arc;

    use super::{candidate_fetch_limit, get_relevant_stored_facts, get_stored_facts};
    use crate::event_bus::EventBus;
    use crate::extract::Claims;
    use crate::state::KnowledgeState;

    fn timestamp(second: i64) -> jiff::Timestamp {
        match jiff::Timestamp::from_second(second) {
            Ok(timestamp) => timestamp,
            Err(error) => panic!("valid test timestamp failed: {error}"),
        }
    }

    fn fact_id(id: &str) -> mneme::id::FactId {
        match mneme::id::FactId::new(id) {
            Ok(id) => id,
            Err(error) => panic!("valid test fact id failed: {error}"),
        }
    }

    fn make_fact(id: &str, content: &str, recorded_at: jiff::Timestamp) -> mneme::knowledge::Fact {
        use mneme::knowledge::{
            EpistemicTier, FactAccess, FactLifecycle, FactProvenance, FactSensitivity,
            FactTemporal, Visibility,
        };

        mneme::knowledge::Fact {
            id: fact_id(id),
            nous_id: "alice".to_owned(),
            fact_type: "knowledge".to_owned(),
            content: content.to_owned(),
            scope: None,
            project_id: None,
            sensitivity: FactSensitivity::Public,
            visibility: Visibility::Private,
            temporal: FactTemporal {
                valid_from: jiff::Timestamp::UNIX_EPOCH,
                valid_to: timestamp(4_102_444_800),
                recorded_at,
            },
            provenance: FactProvenance {
                confidence: 0.9,
                tier: EpistemicTier::Verified,
                source_session_id: None,
                stability_hours: 24.0,
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
        }
    }

    fn knowledge_state(store: Arc<mneme::knowledge_store::KnowledgeStore>) -> KnowledgeState {
        KnowledgeState {
            knowledge_store: Some(store),
            config: Arc::new(tokio::sync::RwLock::new(
                taxis::config::AletheiaConfig::default(),
            )),
            event_bus: Arc::new(EventBus::new(16)),
        }
    }

    #[tokio::test]
    async fn relevance_fetch_returns_fact_beyond_native_cap() {
        let store = match mneme::knowledge_store::KnowledgeStore::open_mem() {
            Ok(store) => store,
            Err(error) => panic!("open knowledge store: {error}"),
        };
        if let Err(error) = store.insert_fact(&make_fact(
            "target-needle",
            "needle launch readiness fact",
            timestamp(1),
        )) {
            panic!("insert target fact: {error}");
        }
        for index in 0..60 {
            let id = format!("recent-{index}");
            let content = format!("recent filler memory {index}");
            if let Err(error) =
                store.insert_fact(&make_fact(&id, &content, timestamp(1_000 + index)))
            {
                panic!("insert recent fact {index}: {error}");
            }
        }

        let state = knowledge_state(Arc::clone(&store));
        let claims = Claims {
            sub: "alice".to_owned(),
            role: symbolon::types::Role::Operator,
            nous_id: None,
        };
        let policy = match super::super::KnowledgeReadPolicy::from_single_nous(&claims, None) {
            Ok(policy) => policy,
            Err(error) => panic!("build policy: {error}"),
        };
        let fetch_limit = candidate_fetch_limit(1);
        let native_query = super::super::FactsQuery {
            nous_id: None,
            sort: super::super::default_sort(),
            order: super::super::default_order(),
            filter: None,
            fact_type: None,
            tier: None,
            limit: fetch_limit,
            offset: 0,
            include_forgotten: true,
        };

        let native = get_stored_facts(&state, &policy, &native_query)
            .await
            .expect("configured store must answer");
        assert!(
            native
                .iter()
                .all(|fact| fact.id.as_str() != "target-needle"),
            "native-sort cap unexpectedly included the old target fact"
        );

        let relevant = get_relevant_stored_facts(&state, &policy, "needle", fetch_limit)
            .await
            .expect("configured store must answer");
        assert!(
            relevant
                .iter()
                .any(|fact| fact.id.as_str() == "target-needle"),
            "store-level text relevance should return the old target fact"
        );
    }
}
