use serde_json::Value;
use snafu::{OptionExt as _, ResultExt as _};

use crate::client::{
    EvalClient, KnowledgeExplainResponse, KnowledgeFact, KnowledgeFactDetail,
    KnowledgeFactsResponse,
};
use crate::error::{self, Result};
use crate::scenario::assert_eval;

#[cfg(test)]
pub(super) const BACKEND_INVARIANT_PREFIX: &str = "Backend invariant:";

pub(super) async fn first_nous_id(client: &EvalClient) -> Result<String> {
    let nous_list = client.list_nous().await?;
    let nous = nous_list
        .first()
        .context(crate::error::NoAgentsAvailableSnafu)?;
    Ok(nous.id.clone())
}

pub(super) fn unique_fact_id(suffix: &str) -> String {
    format!("fact-{}", crate::scenarios::unique_key("canary", suffix))
}

pub(super) fn unique_marker(suffix: &str) -> String {
    crate::scenarios::unique_key("marker", suffix)
}

#[expect(
    clippy::too_many_arguments,
    reason = "test fact fixture mirrors the durable fact wire contract"
)]
pub(super) fn fact_json(
    id: &str,
    nous_id: &str,
    content: &str,
    fact_type: &str,
    tier: &str,
    confidence: f64,
    source_session_id: &str,
    superseded_by: Option<&str>,
) -> Value {
    let now = jiff::Timestamp::now();
    serde_json::json!({
        "id": id,
        "nous_id": nous_id,
        "fact_type": fact_type,
        "content": content,
        "sensitivity": "public",
        "visibility": "private",
        "valid_from": now,
        "valid_to": mneme::knowledge::far_future(),
        "recorded_at": now,
        "confidence": confidence,
        "tier": tier,
        "source_session_id": source_session_id,
        "stability_hours": 720.0,
        "superseded_by": superseded_by,
        "is_forgotten": false,
        "forgotten_at": null,
        "forget_reason": null,
        "access_count": 0,
        "last_accessed_at": null
    })
}

pub(super) fn fact_json_recorded_at(
    id: &str,
    nous_id: &str,
    content: &str,
    source_session_id: &str,
    recorded_at: jiff::Timestamp,
) -> Value {
    serde_json::json!({
        "id": id,
        "nous_id": nous_id,
        "fact_type": "event",
        "content": content,
        "sensitivity": "public",
        "visibility": "private",
        "valid_from": recorded_at,
        "valid_to": mneme::knowledge::far_future(),
        "recorded_at": recorded_at,
        "confidence": 0.95,
        "tier": "verified",
        "source_session_id": source_session_id,
        "stability_hours": 720.0,
        "superseded_by": null,
        "is_forgotten": false,
        "forgotten_at": null,
        "forget_reason": null,
        "access_count": 0,
        "last_accessed_at": null
    })
}

pub(super) async fn ingest_json_facts(
    client: &EvalClient,
    nous_id: &str,
    facts: Vec<Value>,
) -> Result<()> {
    let expected_inserted = facts.len();
    let content = serde_json::to_string(&facts).context(error::JsonSnafu)?;
    let ingest = client.ingest_knowledge(nous_id, &content, "json").await?;
    assert_eval(
        ingest.inserted == expected_inserted,
        format!(
            "knowledge ingest inserted {} facts, expected {}; skipped={}, errors={:?}",
            ingest.inserted, expected_inserted, ingest.skipped, ingest.errors
        ),
    )?;
    assert_eval(
        ingest.skipped == 0 && ingest.errors.is_empty(),
        format!(
            "knowledge ingest should not skip facts; skipped={}, errors={:?}",
            ingest.skipped, ingest.errors
        ),
    )
}

pub(super) async fn facts_for_marker(
    client: &EvalClient,
    nous_id: &str,
    marker: &str,
) -> Result<KnowledgeFactsResponse> {
    client
        .list_knowledge_facts(nous_id, Some(marker), 20, "confidence", "desc", true)
        .await
}

pub(super) async fn facts_for_marker_ordered_by_created(
    client: &EvalClient,
    nous_id: &str,
    marker: &str,
) -> Result<KnowledgeFactsResponse> {
    client
        .list_knowledge_facts(nous_id, Some(marker), 20, "created", "asc", true)
        .await
}

pub(super) fn find_fact<'a>(
    facts: &'a [KnowledgeFactDetail],
    id: &str,
) -> Result<&'a KnowledgeFactDetail> {
    facts
        .iter()
        .find(|fact| fact.id == id)
        .context(error::AssertionSnafu {
            message: format!("durable fact {id:?} was not returned by the facts API"),
        })
}

pub(super) fn find_search_fact<'a>(
    facts: &'a [KnowledgeFact],
    id: &str,
) -> Result<&'a KnowledgeFact> {
    facts
        .iter()
        .find(|fact| fact.id == id)
        .context(error::AssertionSnafu {
            message: format!("fact {id:?} was not selected by knowledge search"),
        })
}

pub(super) fn assert_search_selected(explain: &KnowledgeExplainResponse, id: &str) -> Result<()> {
    let selected = explain.selected.iter().any(|candidate| candidate.id == id);
    assert_eval(
        selected,
        format!(
            "explainable recall did not select fact {id:?}; selected={:?}, dropped={:?}",
            explain.selected, explain.dropped
        ),
    )
}

pub(super) fn assert_fact_provenance(
    fact: &KnowledgeFactDetail,
    expected_source: &str,
    expected_tier: &str,
    minimum_confidence: f64,
) -> Result<()> {
    assert_eval(
        fact.source_session_id.as_deref() == Some(expected_source),
        format!(
            "fact {} source_session_id mismatch: expected {:?}, got {:?}",
            fact.id, expected_source, fact.source_session_id
        ),
    )?;
    assert_eval(
        fact.tier == expected_tier,
        format!(
            "fact {} tier mismatch: expected {expected_tier:?}, got {:?}",
            fact.id, fact.tier
        ),
    )?;
    assert_eval(
        fact.confidence >= minimum_confidence,
        format!(
            "fact {} confidence too low: expected >= {minimum_confidence}, got {}",
            fact.id, fact.confidence
        ),
    )
}
