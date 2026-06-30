use snafu::OptionExt as _;
use tracing::Instrument;

use crate::client::EvalClient;
use crate::scenario::{
    Scenario, ScenarioClassification, ScenarioFuture, ScenarioMeta, assert_eval,
};
use crate::sse;

use super::support::{
    assert_fact_provenance, assert_search_selected, fact_json, facts_for_marker, find_fact,
    find_search_fact, first_nous_id, ingest_json_facts, unique_fact_id, unique_marker,
};

/// Ingest technical knowledge → verify durable search and provenance
pub(super) struct KnowledgeExtractTechnical;

impl Scenario for KnowledgeExtractTechnical {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "canary-knowledge-extract-technical",
            description: "Backend invariant: technical knowledge is durable, searchable by fact ID, and carries provenance",
            category: "canary-knowledge",
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
                    let marker = unique_marker("knowledge-tech");
                    let fact_id = unique_fact_id("knowledge-tech");
                    let evidence_id = format!("{marker}-source");
                    let content = format!(
                        "Kubernetes {marker} is a container orchestration platform that manages Pods and exposes Services."
                    );

                    ingest_json_facts(
                        client,
                        &nous_id,
                        vec![fact_json(
                            &fact_id,
                            &nous_id,
                            &content,
                            "knowledge",
                            "verified",
                            0.97,
                            &evidence_id,
                            None,
                        )],
                    )
                    .await?;

                    let search = client
                        .search_knowledge(&format!("Kubernetes Pods {marker}"), &nous_id, 5)
                        .await?;
                    let search_fact = find_search_fact(&search.facts, &fact_id)?;
                    assert_eval(
                        search_fact.tier == "verified" && search_fact.fact_type == "knowledge",
                        format!(
                            "search result did not expose expected provenance/category: {search_fact:?}"
                        ),
                    )?;
                    assert_eval(
                        search_fact.score > 0.0,
                        format!("search result should carry positive score: {search_fact:?}"),
                    )?;

                    let explain = client
                        .explain_knowledge(&format!("container orchestration {marker}"), &nous_id, 5)
                        .await?;
                    assert_search_selected(&explain, &fact_id)?;

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
                id = "canary-knowledge-extract-technical"
            )),
        )
    }
}

/// Ingest replacement knowledge → verify supersession state
pub(super) struct KnowledgeDetectContradiction;

impl Scenario for KnowledgeDetectContradiction {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "canary-knowledge-detect-contradiction",
            description: "Backend invariant: contradictory replacement state preserves the superseded fact and selects the current fact",
            category: "canary-knowledge",
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
                    let marker = unique_marker("knowledge-contradiction");
                    let old_id = unique_fact_id("knowledge-contradiction-old");
                    let current_id = unique_fact_id("knowledge-contradiction-current");
                    let old_source = format!("{marker}-old-source");
                    let current_source = format!("{marker}-current-source");
                    let old_content =
                        format!("Project Canary {marker} release deadline is March 15.");
                    let current_content =
                        format!("Project Canary {marker} release deadline is April 1.");

                    ingest_json_facts(
                        client,
                        &nous_id,
                        vec![
                            fact_json(
                                &old_id,
                                &nous_id,
                                &old_content,
                                "knowledge",
                                "inferred",
                                0.72,
                                &old_source,
                                Some(&current_id),
                            ),
                            fact_json(
                                &current_id,
                                &nous_id,
                                &current_content,
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
                        .search_knowledge(&format!("Project Canary deadline {marker}"), &nous_id, 5)
                        .await?;
                    find_search_fact(&search.facts, &current_id)?;
                    assert_eval(
                        !search.facts.iter().any(|fact| fact.id == old_id),
                        format!("superseded fact {old_id:?} should not be selected by search"),
                    )?;

                    let facts = facts_for_marker(client, &nous_id, &marker).await?;
                    let old = find_fact(&facts.facts, &old_id)?;
                    let current = find_fact(&facts.facts, &current_id)?;
                    assert_eval(
                        old.superseded_by.as_deref() == Some(current_id.as_str()),
                        format!(
                            "old fact should point at current fact via superseded_by; got {:?}",
                            old.superseded_by
                        ),
                    )?;
                    assert_eval(
                        current.superseded_by.is_none(),
                        format!("current fact should not be superseded: {current:?}"),
                    )?;
                    assert_fact_provenance(current, &current_source, "verified", 0.95)?;

                    Ok(())
                }
                .await;
                result.into()
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "canary-knowledge-detect-contradiction"
            )),
        )
    }
}

/// Ingest update to existing knowledge → verify revision state
pub(super) struct KnowledgeUpdateRevision;

impl Scenario for KnowledgeUpdateRevision {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "canary-knowledge-update-revision",
            description: "Backend invariant: revised knowledge keeps an auditable prior row and retrieves the replacement row",
            category: "canary-knowledge",
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
                    let marker = unique_marker("knowledge-revision");
                    let old_id = unique_fact_id("knowledge-revision-old");
                    let revised_id = unique_fact_id("knowledge-revision-current");
                    let old_source = format!("{marker}-old-source");
                    let revised_source = format!("{marker}-revised-source");

                    ingest_json_facts(
                        client,
                        &nous_id,
                        vec![
                            fact_json(
                                &old_id,
                                &nous_id,
                                &format!("The {marker} project deadline is March 15."),
                                "task",
                                "inferred",
                                0.75,
                                &old_source,
                                Some(&revised_id),
                            ),
                            fact_json(
                                &revised_id,
                                &nous_id,
                                &format!("The {marker} project deadline is April 1."),
                                "task",
                                "verified",
                                0.98,
                                &revised_source,
                                None,
                            ),
                        ],
                    )
                    .await?;

                    let explain = client
                        .explain_knowledge(&format!("{marker} current project deadline"), &nous_id, 5)
                        .await?;
                    assert_search_selected(&explain, &revised_id)?;

                    let facts = facts_for_marker(client, &nous_id, &marker).await?;
                    let old = find_fact(&facts.facts, &old_id)?;
                    let revised = find_fact(&facts.facts, &revised_id)?;
                    assert_eval(
                        old.superseded_by.as_deref() == Some(revised_id.as_str()),
                        format!(
                            "prior revision should point at replacement via superseded_by; got {:?}",
                            old.superseded_by
                        ),
                    )?;
                    assert_eval(
                        revised.content.contains("April 1"),
                        format!("revised fact should preserve replacement content: {revised:?}"),
                    )?;
                    assert_fact_provenance(revised, &revised_source, "verified", 0.95)?;

                    Ok(())
                }
                .await;
                result.into()
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "canary-knowledge-update-revision"
            )),
        )
    }
}

/// Ingest ambiguous knowledge → verify low confidence provenance
pub(super) struct KnowledgeAmbiguousLowConfidence;

impl Scenario for KnowledgeAmbiguousLowConfidence {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "canary-knowledge-ambiguous-low-confidence",
            description: "Backend invariant: uncertain knowledge is stored with assumed tier and low confidence",
            category: "canary-knowledge",
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
                    let marker = unique_marker("knowledge-ambiguous");
                    let fact_id = unique_fact_id("knowledge-ambiguous");
                    let evidence_id = format!("{marker}-source");

                    ingest_json_facts(
                        client,
                        &nous_id,
                        vec![fact_json(
                            &fact_id,
                            &nous_id,
                            &format!(
                                "The ambiguous referent {marker} was described only as pretty good."
                            ),
                            "observation",
                            "assumed",
                            0.35,
                            &evidence_id,
                            None,
                        )],
                    )
                    .await?;

                    let facts = facts_for_marker(client, &nous_id, &marker).await?;
                    let stored = find_fact(&facts.facts, &fact_id)?;
                    assert_eval(
                        stored.tier == "assumed",
                        format!("ambiguous fact should use assumed tier: {stored:?}"),
                    )?;
                    assert_eval(
                        stored.confidence <= 0.40,
                        format!(
                            "ambiguous fact should keep low confidence <= 0.40, got {}",
                            stored.confidence
                        ),
                    )?;
                    assert_fact_provenance(stored, &evidence_id, "assumed", 0.0)?;

                    let search = client
                        .search_knowledge(&format!("ambiguous referent {marker}"), &nous_id, 5)
                        .await?;
                    find_search_fact(&search.facts, &fact_id)?;
                    Ok(())
                }
                .await;
                result.into()
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "canary-knowledge-ambiguous-low-confidence"
            )),
        )
    }
}

/// Send meta-knowledge prompt → smoke-check completed text
pub(super) struct KnowledgeMetaCategorization;

impl Scenario for KnowledgeMetaCategorization {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "canary-knowledge-meta-categorization",
            description: "Smoke: meta-knowledge prompt returns a non-empty response; this does not assert backend categorization",
            category: "canary-knowledge",
            requires_auth: true,
            requires_nous: true,
            expected_contains: None,
            expected_pattern: None,
            classification: ScenarioClassification::Smoke,
        }
    }

    fn run<'a>(&'a self, client: &'a EvalClient) -> ScenarioFuture<'a> {
        Box::pin(
            async move {
                let result: crate::error::Result<()> = async {
                    let nous_list = client.list_nous().await?;
                    let nous = nous_list
                        .first()
                        .context(crate::error::NoAgentsAvailableSnafu)?;
                    let nous_id = &nous.id;
                    let key = crate::scenarios::unique_key("canary", "knowledge-meta");
                    let session = client.create_session(nous_id, &key).await?;

                    let events = client
                        .send_message(
                            &session.id,
                            "This is meta-information: I am testing an AI system. \
                         How do you categorize statements about the \
                         conversation itself vs. factual knowledge?",
                        )
                        .await?;
                    let text = sse::extract_text(&events);
                    assert_eval(
                        !text.is_empty(),
                        "Meta-knowledge response should not be empty",
                    )?;

                    // kanon:ignore RUST/no-silent-result-swallow — session cleanup after canary scenario
                    let _ = client.close_session(&session.id).await;
                    Ok(())
                }
                .await;
                result.into()
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "canary-knowledge-meta-categorization"
            )),
        )
    }
}
