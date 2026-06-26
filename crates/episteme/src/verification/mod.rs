//! Multi-agent verification protocol: publish + vote + conflict-resolution
//! operations on top of `eidos`'s `PublishedFact` / `VerificationProposal` /
//! `ConflictResolution` types.

pub mod conflict;
pub mod proposal;

pub use conflict::{Conflict, ConflictKind, ResolveError, resolve_conflict};
pub use proposal::{
    DEFAULT_VERIFICATION_THRESHOLD, VerificationOutcome, publish_fact, vote_on_proposal,
};

/// Detect whether a newly-extracted fact conflicts with existing facts in the
/// same cohort.
///
/// Uses BM25 text search against the knowledge store, post-filters by `nous_id`
/// to enforce cohort isolation, and classifies exact content matches as
/// [`ConflictKind::Duplicate`] vs different content as
/// [`ConflictKind::Contradiction`].
///
/// # Cohort isolation
///
/// The search is scoped to the provided `nous_id`. Facts belonging to other
/// nouses are explicitly filtered out via Datalog query.
#[cfg(feature = "mneme-engine")]
pub fn detect_conflict(
    fact: &eidos::bookkeeping::ExtractedFact,
    store: &crate::knowledge_store::KnowledgeStore,
    nous_id: &str,
) -> Result<Option<Conflict>, crate::extract::ExtractionError> {
    use std::collections::BTreeMap;

    let content = format!("{} {} {}", fact.subject, fact.predicate, fact.object);
    let results = store.search_text_for_recall(&content, 5).map_err(|e| {
        crate::extract::PersistSnafu {
            message: format!("conflict search failed: {e}"),
        }
        .build()
    })?;

    for result in results {
        if result.source_type != "fact" {
            continue;
        }

        let script = r"
            ?[id, content, nous_id] :=
                *facts{id, content, nous_id, valid_from},
                id == $fact_id
        ";
        let mut params = BTreeMap::new();
        params.insert(
            "fact_id".to_owned(),
            crate::engine::DataValue::Str(result.source_id.as_str().into()),
        );

        let query_result = store.run_query(script, params).map_err(|e| {
            crate::extract::PersistSnafu {
                message: format!("cohort query failed: {e}"),
            }
            .build()
        })?;

        for row in query_result.rows() {
            let row_nous_id = row.get(2).and_then(|v| v.get_str()).unwrap_or("");
            if row_nous_id != nous_id {
                continue;
            }

            let existing_content = row.get(1).and_then(|v| v.get_str()).unwrap_or("");
            let kind = if existing_content == content {
                ConflictKind::Duplicate
            } else {
                ConflictKind::Contradiction
            };

            // WHY: surface conflict/contradiction counts at extraction time so
            // operators can alert on memory-quality spikes, not just throughput.
            crate::metrics::record_extraction_conflict(
                nous_id,
                "detect_conflict",
                "unknown",
                "unknown",
            );
            if kind == ConflictKind::Contradiction {
                crate::metrics::record_extraction_contradiction(
                    nous_id,
                    "detect_conflict",
                    "unknown",
                    "unknown",
                );
            }

            let similarity = (1.0 - result.distance).clamp(0.0, 1.0);

            let incoming = crate::id::FactId::new(format!(
                "{}-{}-new",
                fact.subject.replace(' ', "-"),
                fact.predicate.replace(' ', "-")
            ))
            .map_err(|e| {
                crate::extract::PersistSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;

            let existing = crate::id::FactId::new(result.source_id.clone()).map_err(|e| {
                crate::extract::PersistSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;

            return Ok(Some(Conflict {
                incoming,
                existing,
                kind,
                similarity,
            }));
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests;
