use snafu::OptionExt as _;
use tracing::Instrument;

use crate::client::EvalClient;
use crate::scenario::{
    Scenario, ScenarioClassification, ScenarioFuture, ScenarioMeta, assert_eval,
};
use crate::sse;

/// File read tool capability query smoke test.
pub(super) struct ToolFileReadContent;

impl Scenario for ToolFileReadContent {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "canary-tool-file-read-content",
            description: "File read capability query returns a completed non-empty response",
            category: "canary-tool",
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
                    let key = crate::scenarios::unique_key("canary", "tool-read");
                    let session = client.create_session(nous_id, &key).await?;

                    // Ask to read a standard file - will use tool or respond about capability
                    let events = client
                        .send_message(
                            &session.id,
                            "Use the file read tool to check if /etc/hostname exists. \
                         If you cannot use tools, explain your capabilities.",
                        )
                        .await?;
                    assert_eval(!events.is_empty(), "Tool use should produce events")?;
                    assert_eval(sse::is_complete(&events), "SSE stream should complete")?;
                    let text = sse::extract_text(&events);
                    assert_eval(!text.is_empty(), "Tool use response should not be empty")?;

                    // kanon:ignore RUST/no-silent-result-swallow — session cleanup after canary scenario
                    let _ = client.close_session(&session.id).await;
                    Ok(())
                }
                .await;
                result.into()
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "canary-tool-file-read-content"
            )),
        )
    }
}

/// File write/read capability query smoke test.
pub(super) struct ToolFileWriteReadRoundtrip;

impl Scenario for ToolFileWriteReadRoundtrip {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "canary-tool-file-write-read-roundtrip",
            description: "File write/read capability query returns a non-empty response",
            category: "canary-tool",
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
                    let key = crate::scenarios::unique_key("canary", "tool-roundtrip");
                    let session = client.create_session(nous_id, &key).await?;

                    // Ask about file write capability - system should respond appropriately
                    let events = client
                        .send_message(
                            &session.id,
                            "Can you write files and then read them back? \
                         Explain your file system capabilities.",
                        )
                        .await?;
                    let text = sse::extract_text(&events);
                    assert_eval(
                        !text.is_empty(),
                        "Tool capability response should not be empty",
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
                id = "canary-tool-file-write-read-roundtrip"
            )),
        )
    }
}

/// Web search capability query smoke test.
pub(super) struct ToolWebSearchStructured;

impl Scenario for ToolWebSearchStructured {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "canary-tool-web-search-structured",
            description: "Web search capability query returns a non-empty response",
            category: "canary-tool",
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
                    let key = crate::scenarios::unique_key("canary", "tool-websearch");
                    let session = client.create_session(nous_id, &key).await?;

                    let events = client
                        .send_message(
                            &session.id,
                            "Do you have web search capability? \
                         If yes, how would you search for current news? \
                         If no, explain your limitations.",
                        )
                        .await?;
                    let text = sse::extract_text(&events);
                    assert_eval(!text.is_empty(), "Web search response should not be empty")?;

                    // kanon:ignore RUST/no-silent-result-swallow — session cleanup after canary scenario
                    let _ = client.close_session(&session.id).await;
                    Ok(())
                }
                .await;
                result.into()
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "canary-tool-web-search-structured"
            )),
        )
    }
}

/// Multi-step tool-chain capability query smoke test.
pub(super) struct ToolMultiToolChain;

impl Scenario for ToolMultiToolChain {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "canary-tool-multi-tool-chain",
            description: "Multi-step tool-chain capability query returns a non-empty response",
            category: "canary-tool",
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
                    let key = crate::scenarios::unique_key("canary", "tool-chain");
                    let session = client.create_session(nous_id, &key).await?;

                    let events = client
                        .send_message(
                            &session.id,
                            "Explain how you would handle a multi-step task: \
                         1) Read a file, 2) Transform its contents, 3) Write to a new file. \
                         Describe your tool chaining capabilities.",
                        )
                        .await?;
                    let text = sse::extract_text(&events);
                    assert_eval(!text.is_empty(), "Multi-tool response should not be empty")?;

                    // kanon:ignore RUST/no-silent-result-swallow — session cleanup after canary scenario
                    let _ = client.close_session(&session.id).await;
                    Ok(())
                }
                .await;
                result.into()
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "canary-tool-multi-tool-chain"
            )),
        )
    }
}

/// Tool error-handling capability query smoke test.
pub(super) struct ToolInvalidInputError;

impl Scenario for ToolInvalidInputError {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "canary-tool-invalid-input-error",
            description: "Tool error-handling explanation returns a non-empty response",
            category: "canary-tool",
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
                    let key = crate::scenarios::unique_key("canary", "tool-error");
                    let session = client.create_session(nous_id, &key).await?;

                    let events = client
                        .send_message(
                            &session.id,
                            "How do you handle tool errors or invalid inputs? \
                         Give an example of what happens when a tool call fails.",
                        )
                        .await?;
                    let text = sse::extract_text(&events);
                    assert_eval(
                        !text.is_empty(),
                        "Error handling response should not be empty",
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
                id = "canary-tool-invalid-input-error"
            )),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_canaries_are_labeled_smoke_when_only_asserting_non_empty_text() {
        let metas = [
            ToolFileReadContent.meta(),
            ToolFileWriteReadRoundtrip.meta(),
            ToolWebSearchStructured.meta(),
            ToolMultiToolChain.meta(),
            ToolInvalidInputError.meta(),
        ];

        for meta in metas {
            assert!(
                meta.description.contains("non-empty response")
                    || meta.description.contains("completed non-empty response"),
                "tool canary descriptions should describe the non-empty-response smoke assertion"
            );
            assert_eq!(
                meta.classification,
                ScenarioClassification::Smoke,
                "tool canary {} should not be assertive without observable tool-call invariants",
                meta.id
            );
        }
    }
}
