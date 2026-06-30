use tracing::Instrument;

use crate::client::EvalClient;
use crate::scenario::{
    Scenario, ScenarioClassification, ScenarioFuture, ScenarioMeta, assert_eval,
};

use super::support::{
    assert_fact_provenance, assert_search_selected, fact_json, fact_json_recorded_at,
    facts_for_marker, facts_for_marker_ordered_by_created, find_fact, find_search_fact,
    first_nous_id, ingest_json_facts, unique_fact_id, unique_marker,
};

/// Insert fact → query typed store → verify exact fact ID
pub(super) struct RecallInsertQueryRoundtrip;

impl Scenario for RecallInsertQueryRoundtrip {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "canary-recall-insert-query-roundtrip",
            description: "Backend invariant: inserted memory is durably searchable by exact fact ID and provenance",
            category: "canary-recall",
            requires_auth: true,
            requires_nous: true,
            expected_contains: None,
            expected_pattern: None,

            classification: ScenarioClassification::Assertive,
        }
    }

    fn run<'a>(&'a self, client: &'a EvalClient) -> ScenarioFuture<'a> {
        Box::pin(
            async move {
                let result: crate::error::Result<()> = async {
                    let nous_id = first_nous_id(client).await?;
                    let marker = unique_marker("recall-roundtrip");
                    let fact_id = unique_fact_id("recall-roundtrip");
                    let evidence_id = format!("{marker}-source");
                    let content =
                        format!("The durable recall marker is eval-canary-test-fact {marker}.");

                    ingest_json_facts(
                        client,
                        &nous_id,
                        vec![fact_json(
                            &fact_id,
                            &nous_id,
                            &content,
                            "knowledge",
                            "verified",
                            0.99,
                            &evidence_id,
                            None,
                        )],
                    )
                    .await?;

                    let search = client
                        .search_knowledge(&format!("eval-canary-test-fact {marker}"), &nous_id, 5)
                        .await?;
                    let search_fact = find_search_fact(&search.facts, &fact_id)?;
                    assert_eval(
                        search_fact.content.contains(&marker),
                        format!("selected fact should contain unique marker: {search_fact:?}"),
                    )?;

                    let facts = facts_for_marker(client, &nous_id, &marker).await?;
                    let stored = find_fact(&facts.facts, &fact_id)?;
                    assert_fact_provenance(stored, &evidence_id, "verified", 0.95)?;
                    Ok(())
                }
                .await;
                result.into()
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "canary-recall-insert-query-roundtrip"
            )),
        )
    }
}

/// Insert related facts → verify selected ranking
pub(super) struct RecallSemanticSearch;

impl Scenario for RecallSemanticSearch {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "canary-recall-semantic-search",
            description: "Backend invariant: semantic recall ranks the relevant fact above sibling facts",
            category: "canary-recall",
            requires_auth: true,
            requires_nous: true,
            expected_contains: None,
            expected_pattern: None,

            classification: ScenarioClassification::Assertive,
        }
    }

    fn run<'a>(&'a self, client: &'a EvalClient) -> ScenarioFuture<'a> {
        Box::pin(
            async move {
                let result: crate::error::Result<()> = async {
                    let nous_id = first_nous_id(client).await?;
                    let marker = unique_marker("recall-semantic");
                    let redwood_id = unique_fact_id("recall-semantic-redwood");
                    let sequoia_id = unique_fact_id("recall-semantic-sequoia");
                    let pine_id = unique_fact_id("recall-semantic-pine");
                    let evidence_id = format!("{marker}-source");

                    ingest_json_facts(
                        client,
                        &nous_id,
                        vec![
                            fact_json(
                                &redwood_id,
                                &nous_id,
                                &format!("Redwoods {marker} are the tallest trees."),
                                "knowledge",
                                "verified",
                                0.98,
                                &evidence_id,
                                None,
                            ),
                            fact_json(
                                &sequoia_id,
                                &nous_id,
                                &format!("Sequoias {marker} are the most massive trees."),
                                "knowledge",
                                "verified",
                                0.90,
                                &evidence_id,
                                None,
                            ),
                            fact_json(
                                &pine_id,
                                &nous_id,
                                &format!("Bristlecone pines {marker} are the oldest trees."),
                                "knowledge",
                                "verified",
                                0.88,
                                &evidence_id,
                                None,
                            ),
                        ],
                    )
                    .await?;

                    let search = client
                        .search_knowledge(&format!("tallest trees {marker}"), &nous_id, 5)
                        .await?;
                    let first = search.facts.first().ok_or_else(|| {
                        crate::error::AssertionSnafu {
                            message: "semantic search returned no facts".to_owned(),
                        }
                        .build()
                    })?;
                    assert_eval(
                        first.id == redwood_id,
                        format!("semantic search should rank redwood first; got {search:?}"),
                    )?;
                    assert_eval(
                        search.facts.iter().any(|fact| fact.id == sequoia_id)
                            && search.facts.iter().any(|fact| fact.id == pine_id),
                        format!("semantic search should keep sibling facts visible: {search:?}"),
                    )?;

                    let explain = client
                        .explain_knowledge(&format!("tallest trees {marker}"), &nous_id, 5)
                        .await?;
                    assert_search_selected(&explain, &redwood_id)?;
                    Ok(())
                }
                .await;
                result.into()
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "canary-recall-semantic-search"
            )),
        )
    }
}

/// Insert superseded conflict pair → verify current recall and audit state
pub(super) struct RecallConflictDetection;

impl Scenario for RecallConflictDetection {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "canary-recall-conflict-detection",
            description: "Backend invariant: superseded conflicting memory is retained for audit but excluded from current recall",
            category: "canary-recall",
            requires_auth: true,
            requires_nous: true,
            expected_contains: None,
            expected_pattern: None,

            classification: ScenarioClassification::Assertive,
        }
    }

    fn run<'a>(&'a self, client: &'a EvalClient) -> ScenarioFuture<'a> {
        Box::pin(
            async move {
                let result: crate::error::Result<()> = async {
                    let nous_id = first_nous_id(client).await?;
                    let marker = unique_marker("recall-conflict");
                    let old_id = unique_fact_id("recall-conflict-old");
                    let current_id = unique_fact_id("recall-conflict-current");
                    let old_source = format!("{marker}-old-source");
                    let current_source = format!("{marker}-current-source");

                    ingest_json_facts(
                        client,
                        &nous_id,
                        vec![
                            fact_json(
                                &old_id,
                                &nous_id,
                                &format!("The capital canary {marker} answer is Paris."),
                                "knowledge",
                                "inferred",
                                0.70,
                                &old_source,
                                Some(&current_id),
                            ),
                            fact_json(
                                &current_id,
                                &nous_id,
                                &format!("The capital canary {marker} corrected answer is Lyon."),
                                "knowledge",
                                "verified",
                                0.96,
                                &current_source,
                                None,
                            ),
                        ],
                    )
                    .await?;

                    let search = client
                        .search_knowledge(
                            &format!("capital canary corrected {marker}"),
                            &nous_id,
                            5,
                        )
                        .await?;
                    find_search_fact(&search.facts, &current_id)?;
                    assert_eval(
                        !search.facts.iter().any(|fact| fact.id == old_id),
                        format!("superseded conflict fact should not be selected: {search:?}"),
                    )?;

                    let facts = facts_for_marker(client, &nous_id, &marker).await?;
                    let old = find_fact(&facts.facts, &old_id)?;
                    assert_eval(
                        old.superseded_by.as_deref() == Some(current_id.as_str()),
                        format!("conflict audit row should point at current fact: {old:?}"),
                    )?;
                    Ok(())
                }
                .await;
                result.into()
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "canary-recall-conflict-detection"
            )),
        )
    }
}

/// Insert temporal facts → verify durable chronological ordering
pub(super) struct RecallTemporalOrdering;

impl Scenario for RecallTemporalOrdering {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "canary-recall-temporal-ordering",
            description: "Backend invariant: fact listing preserves chronological order by recorded_at",
            category: "canary-recall",
            requires_auth: true,
            requires_nous: true,
            expected_contains: None,
            expected_pattern: None,

            classification: ScenarioClassification::Assertive,
        }
    }

    fn run<'a>(&'a self, client: &'a EvalClient) -> ScenarioFuture<'a> {
        Box::pin(
            async move {
                let result: crate::error::Result<()> = async {
                    let nous_id = first_nous_id(client).await?;
                    let marker = unique_marker("recall-temporal");
                    let moon_id = unique_fact_id("recall-temporal-moon");
                    let wall_id = unique_fact_id("recall-temporal-wall");
                    let union_id = unique_fact_id("recall-temporal-union");
                    let evidence_id = format!("{marker}-source");

                    let moon_time = timestamp("1969-07-20T20:17:40Z")?;
                    let wall_time = timestamp("1989-11-09T00:00:00Z")?;
                    let union_time = timestamp("1991-12-26T00:00:00Z")?;

                    ingest_json_facts(
                        client,
                        &nous_id,
                        vec![
                            fact_json_recorded_at(
                                &wall_id,
                                &nous_id,
                                &format!("In 1989, the Berlin Wall fell {marker}."),
                                &evidence_id,
                                wall_time,
                            ),
                            fact_json_recorded_at(
                                &moon_id,
                                &nous_id,
                                &format!("In 1969, humans first landed on the moon {marker}."),
                                &evidence_id,
                                moon_time,
                            ),
                            fact_json_recorded_at(
                                &union_id,
                                &nous_id,
                                &format!("In 1991, the Soviet Union dissolved {marker}."),
                                &evidence_id,
                                union_time,
                            ),
                        ],
                    )
                    .await?;

                    let facts =
                        facts_for_marker_ordered_by_created(client, &nous_id, &marker).await?;
                    let ordered_ids: Vec<&str> =
                        facts.facts.iter().map(|fact| fact.id.as_str()).collect();
                    assert_eval(
                        ordered_ids == vec![moon_id.as_str(), wall_id.as_str(), union_id.as_str()],
                        format!("facts should be ordered chronologically; got {ordered_ids:?}"),
                    )?;
                    for fact_id in [&moon_id, &wall_id, &union_id] {
                        let fact = find_fact(&facts.facts, fact_id)?;
                        assert_fact_provenance(fact, &evidence_id, "verified", 0.95)?;
                    }

                    let search = client
                        .search_knowledge(&format!("Berlin Wall {marker}"), &nous_id, 5)
                        .await?;
                    find_search_fact(&search.facts, &wall_id)?;
                    Ok(())
                }
                .await;
                result.into()
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "canary-recall-temporal-ordering"
            )),
        )
    }
}

/// Search with empty knowledge → verify graceful empty result
pub(super) struct RecallEmptyKnowledgeGraceful;

impl Scenario for RecallEmptyKnowledgeGraceful {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "canary-recall-empty-knowledge-graceful",
            description: "Backend invariant: empty knowledge query returns no selected facts",
            category: "canary-recall",
            requires_auth: true,
            requires_nous: true,
            expected_contains: None,
            expected_pattern: None,

            classification: ScenarioClassification::Assertive,
        }
    }

    fn run<'a>(&'a self, client: &'a EvalClient) -> ScenarioFuture<'a> {
        Box::pin(
            async move {
                let result: crate::error::Result<()> = async {
                    let nous_id = first_nous_id(client).await?;
                    let marker = unique_marker("recall-empty");
                    let search = client
                        .search_knowledge(
                            &format!("no stored fact should match {marker}"),
                            &nous_id,
                            5,
                        )
                        .await?;
                    assert_eval(
                        search.facts.is_empty(),
                        format!("empty knowledge query should return no facts: {search:?}"),
                    )?;

                    Ok(())
                }
                .await;
                result.into()
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "canary-recall-empty-knowledge-graceful"
            )),
        )
    }
}

fn timestamp(value: &str) -> crate::error::Result<jiff::Timestamp> {
    value.parse().map_err(|e| {
        crate::error::AssertionSnafu {
            message: format!("invalid canary timestamp {value:?}: {e}"),
        }
        .build()
    })
}
