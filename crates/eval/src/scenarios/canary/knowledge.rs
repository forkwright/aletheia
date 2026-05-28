use snafu::OptionExt as _;
use tracing::Instrument;

use crate::client::EvalClient;
use crate::scenario::{Scenario, ScenarioFuture, ScenarioMeta, assert_eval, validate_response};
use crate::sse;

// ---------------------------------------------------------------------------

/// Send technical description → verify fact extraction
pub(super) struct KnowledgeExtractTechnical;

impl Scenario for KnowledgeExtractTechnical {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "canary-knowledge-extract-technical",
            description: "Technical content yields structured fact extraction",
            category: "canary-knowledge",
            requires_auth: true,
            requires_nous: true,
            expected_contains: None,
            expected_pattern: Some(r"(?i)(kubernetes|k8s|container|pod|docker)"),
        }
    }

    fn run<'a>(&'a self, client: &'a EvalClient) -> ScenarioFuture<'a> {
        Box::pin(
            async move {
                let nous_list = client.list_nous().await?;
                let nous = nous_list
                    .first()
                    .context(crate::error::NoAgentsAvailableSnafu)?;
                let nous_id = &nous.id;
                let key = crate::scenarios::unique_key("canary", "knowledge-tech");
                let session = client.create_session(nous_id, &key).await?;

                let events = client
                    .send_message(
                        &session.id,
                        "Kubernetes is a container orchestration platform. \
                         It manages Pods, which are groups of containers. \
                         Services expose Pods to network traffic. \
                         What is Kubernetes and what does it manage?",
                    )
                    .await?;
                let text = sse::extract_text(&events);
                validate_response(&self.meta(), &text)?;

                // kanon:ignore RUST/no-silent-result-swallow — session cleanup after canary scenario
                let _ = client.close_session(&session.id).await;
                Ok(())
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "canary-knowledge-extract-technical"
            )),
        )
    }
}

/// Send contradiction → verify conflict flagged
pub(super) struct KnowledgeDetectContradiction;

impl Scenario for KnowledgeDetectContradiction {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "canary-knowledge-detect-contradiction",
            description: "Contradictory statements are detected and flagged",
            category: "canary-knowledge",
            requires_auth: true,
            requires_nous: true,
            expected_contains: None,
            expected_pattern: Some(r"(?i)(contradict|inconsistent|conflict|cannot both be true)"),
        }
    }

    fn run<'a>(&'a self, client: &'a EvalClient) -> ScenarioFuture<'a> {
        Box::pin(
            async move {
                let nous_list = client.list_nous().await?;
                let nous = nous_list
                    .first()
                    .context(crate::error::NoAgentsAvailableSnafu)?;
                let nous_id = &nous.id;
                let key = crate::scenarios::unique_key("canary", "knowledge-contradiction");
                let session = client.create_session(nous_id, &key).await?;

                let events = client
                    .send_message(
                        &session.id,
                        "I will tell you two things: \
                         1) All birds can fly. \
                         2) Penguins are birds that cannot fly. \
                         Analyze these statements.",
                    )
                    .await?;
                let text = sse::extract_text(&events);
                validate_response(&self.meta(), &text)?;

                // kanon:ignore RUST/no-silent-result-swallow — session cleanup after canary scenario
                let _ = client.close_session(&session.id).await;
                Ok(())
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "canary-knowledge-detect-contradiction"
            )),
        )
    }
}

/// Send update to existing knowledge → verify revision
pub(super) struct KnowledgeUpdateRevision;

impl Scenario for KnowledgeUpdateRevision {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "canary-knowledge-update-revision",
            description: "Knowledge updates properly revise existing facts",
            category: "canary-knowledge",
            requires_auth: true,
            requires_nous: true,
            expected_contains: None,
            expected_pattern: Some(r"(?i)(updated|revised|corrected|new information|changed)"),
        }
    }

    fn run<'a>(&'a self, client: &'a EvalClient) -> ScenarioFuture<'a> {
        Box::pin(
            async move {
                let nous_list = client.list_nous().await?;
                let nous = nous_list
                    .first()
                    .context(crate::error::NoAgentsAvailableSnafu)?;
                let nous_id = &nous.id;
                let key = crate::scenarios::unique_key("canary", "knowledge-revision");
                let session = client.create_session(nous_id, &key).await?;

                let _ = client
                    .send_message(&session.id, "The project deadline is March 15.")
                    .await?;
                let events = client
                    .send_message(
                        &session.id,
                        "Correction: The project deadline is now April 1. \
                         What is the current deadline?",
                    )
                    .await?;
                let text = sse::extract_text(&events);
                validate_response(&self.meta(), &text)?;

                // kanon:ignore RUST/no-silent-result-swallow — session cleanup after canary scenario
                let _ = client.close_session(&session.id).await;
                Ok(())
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "canary-knowledge-update-revision"
            )),
        )
    }
}

/// Send ambiguous statement → verify low confidence
pub(super) struct KnowledgeAmbiguousLowConfidence;

impl Scenario for KnowledgeAmbiguousLowConfidence {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "canary-knowledge-ambiguous-low-confidence",
            description: "Ambiguous input is handled with appropriate uncertainty",
            category: "canary-knowledge",
            requires_auth: true,
            requires_nous: true,
            expected_contains: None,
            expected_pattern: Some(r"(?i)(unclear|ambiguous|not sure|could mean|unsure|uncertain)"),
        }
    }

    fn run<'a>(&'a self, client: &'a EvalClient) -> ScenarioFuture<'a> {
        Box::pin(
            async move {
                let nous_list = client.list_nous().await?;
                let nous = nous_list
                    .first()
                    .context(crate::error::NoAgentsAvailableSnafu)?;
                let nous_id = &nous.id;
                let key = crate::scenarios::unique_key("canary", "knowledge-ambiguous");
                let session = client.create_session(nous_id, &key).await?;

                let events = client
                    .send_message(
                        &session.id,
                        "It was pretty good. \
                         (Note: I haven't said what 'it' refers to. \
                         How do you handle this ambiguity?)",
                    )
                    .await?;
                let text = sse::extract_text(&events);
                validate_response(&self.meta(), &text)?;

                // kanon:ignore RUST/no-silent-result-swallow — session cleanup after canary scenario
                let _ = client.close_session(&session.id).await;
                Ok(())
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "canary-knowledge-ambiguous-low-confidence"
            )),
        )
    }
}

/// Send meta-knowledge (about the system) → verify categorization
pub(super) struct KnowledgeMetaCategorization;

impl Scenario for KnowledgeMetaCategorization {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "canary-knowledge-meta-categorization",
            description: "Meta-knowledge about the system is properly categorized",
            category: "canary-knowledge",
            requires_auth: true,
            requires_nous: true,
            expected_contains: None,
            expected_pattern: None,
        }
    }

    fn run<'a>(&'a self, client: &'a EvalClient) -> ScenarioFuture<'a> {
        Box::pin(
            async move {
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
            .instrument(tracing::info_span!(
                "scenario",
                id = "canary-knowledge-meta-categorization"
            )),
        )
    }
}

// ---------------------------------------------------------------------------
