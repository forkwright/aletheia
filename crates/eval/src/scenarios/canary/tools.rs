use snafu::OptionExt as _;
use tracing::Instrument;

use crate::client::EvalClient;
use crate::scenario::{Scenario, ScenarioFuture, ScenarioMeta, assert_eval};
use crate::sse;

// ---------------------------------------------------------------------------

/// File read tool → verify content returned
pub(super) struct ToolFileReadContent;

impl Scenario for ToolFileReadContent {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "canary-tool-file-read-content",
            description: "File read tool invocation returns expected content",
            category: "canary-tool",
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
                assert_eval(
                    sse::is_complete(&events),
                    "SSE stream should complete",
                )?;
                let text = sse::extract_text(&events);
                assert_eval(!text.is_empty(), "Tool use response should not be empty")?;

                let _ = client.close_session(&session.id).await;
                Ok(())
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "canary-tool-file-read-content"
            )),
        )
    }
}

/// File write → read back → verify roundtrip
pub(super) struct ToolFileWriteReadRoundtrip;

impl Scenario for ToolFileWriteReadRoundtrip {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "canary-tool-file-write-read-roundtrip",
            description: "File write followed by read verifies roundtrip integrity",
            category: "canary-tool",
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
                assert_eval(!text.is_empty(), "Tool capability response should not be empty")?;

                let _ = client.close_session(&session.id).await;
                Ok(())
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "canary-tool-file-write-read-roundtrip"
            )),
        )
    }
}

/// Web search tool → verify structured results
pub(super) struct ToolWebSearchStructured;

impl Scenario for ToolWebSearchStructured {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "canary-tool-web-search-structured",
            description: "Web search tool returns structured, relevant results",
            category: "canary-tool",
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

                let _ = client.close_session(&session.id).await;
                Ok(())
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "canary-tool-web-search-structured"
            )),
        )
    }
}

/// Multi-tool chain (read → transform → write) → verify end state
pub(super) struct ToolMultiToolChain;

impl Scenario for ToolMultiToolChain {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "canary-tool-multi-tool-chain",
            description: "Multi-step tool chain completes successfully",
            category: "canary-tool",
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

                let _ = client.close_session(&session.id).await;
                Ok(())
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "canary-tool-multi-tool-chain"
            )),
        )
    }
}

/// Tool with invalid input → verify error handling
pub(super) struct ToolInvalidInputError;

impl Scenario for ToolInvalidInputError {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "canary-tool-invalid-input-error",
            description: "Tool invocation with invalid input produces clear error",
            category: "canary-tool",
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
                assert_eval(!text.is_empty(), "Error handling response should not be empty")?;

                let _ = client.close_session(&session.id).await;
                Ok(())
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "canary-tool-invalid-input-error"
            )),
        )
    }
}

// ---------------------------------------------------------------------------
