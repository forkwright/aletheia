//! NDJSON wire protocol types and event stream parser.
//!
//! Parses the `claude --output-format stream-json` NDJSON protocol into
//! [`SessionEvent`] values. Wire types mirror the Claude CLI protocol from
//! phronesis: System, Assistant (with content blocks), Result, and `RateLimit`.

use std::collections::VecDeque;

use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::ChildStdout;

use crate::engine::SessionEvent;

// ---------------------------------------------------------------------------
// Wire protocol types (NDJSON from claude CLI)
// ---------------------------------------------------------------------------

/// Top-level NDJSON message from `claude --output-format stream-json`.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub(crate) enum WireMessage {
    /// Session initialization.
    #[serde(rename = "system")]
    System(SystemMessage),
    /// Streamed assistant response with content blocks.
    #[serde(rename = "assistant")]
    Assistant(AssistantMessage),
    /// Final session result.
    #[serde(rename = "result")]
    Result(ResultMessage),
    /// Rate limit status update.
    #[serde(rename = "rate_limit_event")]
    RateLimit(RateLimitMessage),
    /// User turn visible during session resume (deserialized, not consumed).
    // WHY: serde needs somewhere to put "human" messages during session
    // resume replay. We absorb unknown fields silently and discard.
    #[serde(rename = "human")]
    Human {},
    /// Unrecognized message type, ignored.
    // WHY: Claude CLI may emit types we don't handle. Without this catch-all,
    // serde fails on unknown types and kills the session.
    #[serde(other)]
    Unknown,
}

/// System message at session start or compaction boundary.
#[derive(Debug, Deserialize)]
pub(crate) struct SystemMessage {
    #[serde(default)]
    pub(crate) session_id: Option<String>,
}

/// Streamed content from the assistant.
#[derive(Debug, Deserialize)]
pub(crate) struct AssistantMessage {
    #[serde(default)]
    pub(crate) content: Vec<ContentBlock>,
}

/// Individual content block within an assistant message.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub(crate) enum ContentBlock {
    /// Plain text output.
    #[serde(rename = "text")]
    Text { text: String },
    /// Extended thinking output (not surfaced as events).
    #[serde(rename = "thinking")]
    Thinking {},
    /// Tool invocation requested by the assistant.
    #[serde(rename = "tool_use")]
    ToolUse {
        name: String,
        input: serde_json::Value,
    },
    /// Result returned from a tool invocation (not surfaced as events).
    // WHY: ToolResult blocks in the wire protocol represent tool outputs
    // replayed during session resume. The SessionEvent::ToolResult expects
    // a name+success pair which isn't available from the wire content block.
    #[serde(rename = "tool_result")]
    ToolResult {},
}

/// Rate limit status update.
#[derive(Debug, Deserialize)]
pub(crate) struct RateLimitMessage {
    #[serde(default)]
    pub(crate) rate_limit_info: Option<RateLimitInfo>,
}

/// Inner rate limit details.
#[derive(Debug, Deserialize)]
pub(crate) struct RateLimitInfo {
    /// 0.0 to 1.0 -- fraction of rate limit consumed.
    #[serde(default)]
    pub(crate) utilization: Option<f64>,
}

/// Final message when session ends.
#[derive(Debug, Deserialize)]
pub(crate) struct ResultMessage {
    // WHY: Deserialized from wire protocol for test assertions and future use;
    // the session handle uses its own tracked session_id from the System message.
    #[expect(
        dead_code,
        reason = "deserialized from wire but read via EventStream.session_id"
    )]
    pub(crate) session_id: String,
    #[serde(default)]
    pub(crate) result: Option<String>,
    #[serde(default)]
    pub(crate) subtype: String,
    #[serde(default)]
    pub(crate) is_error: bool,
    #[serde(default)]
    pub(crate) total_cost_usd: Option<f64>,
    #[serde(default)]
    pub(crate) num_turns: u32,
    #[serde(default)]
    pub(crate) duration_ms: u64,
}

// ---------------------------------------------------------------------------
// Event stream
// ---------------------------------------------------------------------------

/// Parsed NDJSON stream that yields [`SessionEvent`] values.
///
/// Reads line-delimited JSON from the claude CLI subprocess stdout, parses wire
/// messages, and maps content blocks to individual `SessionEvent` values.
/// Buffers multiple events from a single assistant message (which can contain
/// multiple content blocks).
pub(crate) struct EventStream {
    reader: BufReader<ChildStdout>,
    pending: VecDeque<SessionEvent>,
    /// Session ID captured from the System init message.
    pub(crate) session_id: Option<String>,
    /// Stored result message for `wait()`.
    pub(crate) wire_result: Option<ResultMessage>,
    /// Set when rate limit utilization exceeds 98%.
    pub(crate) rate_limit_exceeded: bool,
}

impl EventStream {
    /// Create a new event stream from subprocess stdout.
    pub(crate) fn new(stdout: ChildStdout) -> Self {
        Self {
            reader: BufReader::new(stdout),
            pending: VecDeque::new(),
            session_id: None,
            wire_result: None,
            rate_limit_exceeded: false,
        }
    }

    /// Yield the next [`SessionEvent`].
    ///
    /// Returns `None` when the stream is exhausted (result received, rate limit
    /// exceeded, or subprocess stdout closed).
    pub(crate) async fn next_event(&mut self) -> Option<SessionEvent> {
        // Drain buffered events from multi-block assistant messages first.
        if let Some(event) = self.pending.pop_front() {
            return Some(event);
        }

        loop {
            let mut line = String::new();
            match self.reader.read_line(&mut line).await {
                Ok(0) => return None,
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(error = %e, "I/O error reading NDJSON stream");
                    return Some(SessionEvent::Error {
                        message: format!("I/O error: {e}"),
                    });
                }
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let msg: WireMessage = match serde_json::from_str(trimmed) {
                Ok(msg) => msg,
                Err(e) => {
                    tracing::warn!(
                        line = %trimmed,
                        error = %e,
                        "skipping unparseable NDJSON line"
                    );
                    continue;
                }
            };

            match msg {
                WireMessage::System(sys) => {
                    if let Some(id) = sys.session_id {
                        self.session_id = Some(id);
                    }
                }
                WireMessage::Assistant(asst) => {
                    self.map_content_blocks(asst.content);
                    if let Some(event) = self.pending.pop_front() {
                        return Some(event);
                    }
                }
                WireMessage::Result(result) => {
                    if result.is_error {
                        self.pending.push_back(SessionEvent::Error {
                            message: result
                                .result
                                .clone()
                                .unwrap_or_else(|| result.subtype.clone()),
                        });
                    }
                    self.wire_result = Some(result);
                    return self.pending.pop_front();
                }
                WireMessage::RateLimit(rl) => {
                    let utilization = rl
                        .rate_limit_info
                        .and_then(|info| info.utilization)
                        .unwrap_or(0.0);
                    if utilization > 0.98 {
                        tracing::warn!(
                            utilization,
                            "rate limit utilization >98%, aborting session"
                        );
                        self.rate_limit_exceeded = true;
                        return None;
                    }
                }
                WireMessage::Human { .. } | WireMessage::Unknown => {}
            }
        }
    }

    /// Map assistant content blocks to buffered `SessionEvent` values.
    fn map_content_blocks(&mut self, blocks: Vec<ContentBlock>) {
        for block in blocks {
            match block {
                ContentBlock::Text { text } => {
                    self.pending.push_back(SessionEvent::TextDelta { text });
                }
                ContentBlock::ToolUse { name, input } => {
                    self.pending
                        .push_back(SessionEvent::ToolUse { name, input });
                }
                // WHY: Thinking and ToolResult blocks are not surfaced as events.
                // Thinking is internal reasoning; ToolResult is replayed during
                // session resume and lacks the name+success pair SessionEvent needs.
                ContentBlock::Thinking { .. } | ContentBlock::ToolResult { .. } => {}
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "test assertions and helpers"
)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_system_init() {
        let json = r#"{"type":"system","subtype":"init","session_id":"sess-001"}"#;
        let msg: WireMessage = serde_json::from_str(json).unwrap();
        assert!(
            matches!(msg, WireMessage::System(sys) if sys.session_id.as_deref() == Some("sess-001"))
        );
    }

    #[test]
    fn deserialize_system_without_session_id() {
        let json = r#"{"type":"system","subtype":"compact_boundary"}"#;
        let msg: WireMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, WireMessage::System(sys) if sys.session_id.is_none()));
    }

    #[test]
    fn deserialize_assistant_text() {
        let json = r#"{"type":"assistant","content":[{"type":"text","text":"Hello"}]}"#;
        let msg: WireMessage = serde_json::from_str(json).unwrap();
        match msg {
            WireMessage::Assistant(asst) => {
                assert_eq!(asst.content.len(), 1);
                assert!(matches!(&asst.content[0], ContentBlock::Text { text } if text == "Hello"));
            }
            _ => panic!("expected Assistant"),
        }
    }

    #[test]
    fn deserialize_assistant_tool_use() {
        let json = r#"{"type":"assistant","content":[{"type":"tool_use","id":"tu-1","name":"bash","input":{"cmd":"ls"}}]}"#;
        let msg: WireMessage = serde_json::from_str(json).unwrap();
        match msg {
            WireMessage::Assistant(asst) => match &asst.content[0] {
                ContentBlock::ToolUse { name, input } => {
                    assert_eq!(name, "bash");
                    assert_eq!(input["cmd"], "ls");
                }
                _ => panic!("expected ToolUse"),
            },
            _ => panic!("expected Assistant"),
        }
    }

    #[test]
    fn deserialize_assistant_multi_blocks() {
        let json = r#"{"type":"assistant","content":[{"type":"thinking","thinking":"hmm"},{"type":"text","text":"answer"},{"type":"tool_use","id":"t1","name":"read","input":{}}]}"#;
        let msg: WireMessage = serde_json::from_str(json).unwrap();
        match msg {
            WireMessage::Assistant(asst) => {
                assert_eq!(asst.content.len(), 3);
                assert!(matches!(&asst.content[0], ContentBlock::Thinking { .. }));
                assert!(matches!(&asst.content[1], ContentBlock::Text { .. }));
                assert!(matches!(&asst.content[2], ContentBlock::ToolUse { .. }));
            }
            _ => panic!("expected Assistant"),
        }
    }

    #[test]
    fn deserialize_assistant_empty_content() {
        let json = r#"{"type":"assistant"}"#;
        let msg: WireMessage = serde_json::from_str(json).unwrap();
        match msg {
            WireMessage::Assistant(asst) => assert!(asst.content.is_empty()),
            _ => panic!("expected Assistant"),
        }
    }

    #[test]
    fn deserialize_result_success() {
        let json = r#"{"type":"result","session_id":"sess-001","subtype":"success","is_error":false,"total_cost_usd":0.42,"num_turns":5,"duration_ms":12345,"result":"Done!"}"#;
        let msg: WireMessage = serde_json::from_str(json).unwrap();
        match msg {
            WireMessage::Result(res) => {
                assert_eq!(res.subtype, "success");
                assert!(!res.is_error);
                assert!((res.total_cost_usd.unwrap() - 0.42).abs() < f64::EPSILON);
                assert_eq!(res.num_turns, 5);
                assert_eq!(res.duration_ms, 12345);
                assert_eq!(res.result.as_deref(), Some("Done!"));
            }
            _ => panic!("expected Result"),
        }
    }

    #[test]
    fn deserialize_result_error() {
        let json = r#"{"type":"result","session_id":"sess-err","subtype":"error_during_execution","is_error":true,"num_turns":1,"duration_ms":500}"#;
        let msg: WireMessage = serde_json::from_str(json).unwrap();
        match msg {
            WireMessage::Result(res) => {
                assert!(res.is_error);
                assert_eq!(res.subtype, "error_during_execution");
                assert!(res.total_cost_usd.is_none());
            }
            _ => panic!("expected Result"),
        }
    }

    #[test]
    fn deserialize_result_minimal() {
        let json = r#"{"type":"result","session_id":"sess-min","subtype":"success"}"#;
        let msg: WireMessage = serde_json::from_str(json).unwrap();
        match msg {
            WireMessage::Result(res) => {
                assert!(!res.is_error);
                assert_eq!(res.num_turns, 0);
                assert_eq!(res.duration_ms, 0);
                assert!(res.total_cost_usd.is_none());
            }
            _ => panic!("expected Result"),
        }
    }

    #[test]
    fn deserialize_rate_limit() {
        let json = r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","utilization":0.45}}"#;
        let msg: WireMessage = serde_json::from_str(json).unwrap();
        match msg {
            WireMessage::RateLimit(rl) => {
                let info = rl.rate_limit_info.unwrap();
                assert!((info.utilization.unwrap() - 0.45).abs() < f64::EPSILON);
            }
            _ => panic!("expected RateLimit"),
        }
    }

    #[test]
    fn deserialize_human_message() {
        let json = r#"{"type":"human","content":[{"type":"text","text":"fix it"}]}"#;
        let msg: WireMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, WireMessage::Human { .. }));
    }

    #[test]
    fn deserialize_unknown_type_ignored() {
        let json = r#"{"type":"something_new","data":123}"#;
        let msg: WireMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, WireMessage::Unknown));
    }

    #[tokio::test]
    async fn map_content_blocks_text_and_tool_use() {
        let blocks = vec![
            ContentBlock::Text {
                text: "hello".to_owned(),
            },
            ContentBlock::ToolUse {
                name: "bash".to_owned(),
                input: serde_json::json!({"cmd": "ls"}),
            },
        ];

        let stdout = make_empty_stdout();
        let mut stream = EventStream::new(stdout);
        stream.map_content_blocks(blocks);

        assert_eq!(stream.pending.len(), 2);
        assert!(matches!(&stream.pending[0], SessionEvent::TextDelta { text } if text == "hello"));
        assert!(matches!(&stream.pending[1], SessionEvent::ToolUse { name, .. } if name == "bash"));
    }

    #[tokio::test]
    async fn map_content_blocks_skips_thinking_and_tool_result() {
        let blocks = vec![
            ContentBlock::Thinking {},
            ContentBlock::ToolResult {},
            ContentBlock::Text {
                text: "answer".to_owned(),
            },
        ];

        let stdout = make_empty_stdout();
        let mut stream = EventStream::new(stdout);
        stream.map_content_blocks(blocks);

        assert_eq!(stream.pending.len(), 1);
        assert!(matches!(&stream.pending[0], SessionEvent::TextDelta { text } if text == "answer"));
    }

    #[tokio::test]
    async fn event_stream_parses_full_session() {
        let ndjson = [
            r#"{"type":"system","subtype":"init","session_id":"sess-test"}"#,
            r#"{"type":"assistant","content":[{"type":"text","text":"Hello"}]}"#,
            r#"{"type":"assistant","content":[{"type":"tool_use","id":"t1","name":"bash","input":{"cmd":"ls"}}]}"#,
            r#"{"type":"result","session_id":"sess-test","subtype":"success","is_error":false,"total_cost_usd":0.10,"num_turns":3,"duration_ms":5000,"result":"Done"}"#,
        ]
        .join("\n");

        let mut stream = event_stream_from_bytes(ndjson.as_bytes());

        let e1 = stream.next_event().await;
        assert!(matches!(e1, Some(SessionEvent::TextDelta { ref text }) if text == "Hello"));

        let e2 = stream.next_event().await;
        assert!(matches!(e2, Some(SessionEvent::ToolUse { ref name, .. }) if name == "bash"));

        let e3 = stream.next_event().await;
        assert!(e3.is_none(), "result message ends the stream");

        assert_eq!(stream.session_id.as_deref(), Some("sess-test"));
        assert!(stream.wire_result.is_some());
        assert_eq!(stream.wire_result.as_ref().unwrap().num_turns, 3);
    }

    #[tokio::test]
    async fn event_stream_skips_malformed_lines() {
        let ndjson = [
            r#"{"type":"system","subtype":"init","session_id":"sess-bad"}"#,
            "NOT JSON",
            "",
            r#"{"type":"assistant","content":[{"type":"text","text":"ok"}]}"#,
            r#"{"type":"result","session_id":"sess-bad","subtype":"success","is_error":false,"num_turns":1,"duration_ms":100}"#,
        ]
        .join("\n");

        let mut stream = event_stream_from_bytes(ndjson.as_bytes());

        let e1 = stream.next_event().await;
        assert!(matches!(e1, Some(SessionEvent::TextDelta { ref text }) if text == "ok"));

        let e2 = stream.next_event().await;
        assert!(e2.is_none());
    }

    #[tokio::test]
    async fn event_stream_rate_limit_abort() {
        let ndjson = [
            r#"{"type":"system","subtype":"init","session_id":"sess-rl"}"#,
            r#"{"type":"assistant","content":[{"type":"text","text":"working"}]}"#,
            r#"{"type":"rate_limit_event","rate_limit_info":{"status":"throttled","utilization":0.99}}"#,
            r#"{"type":"assistant","content":[{"type":"text","text":"should not see"}]}"#,
        ]
        .join("\n");

        let mut stream = event_stream_from_bytes(ndjson.as_bytes());

        let e1 = stream.next_event().await;
        assert!(matches!(e1, Some(SessionEvent::TextDelta { ref text }) if text == "working"));

        let e2 = stream.next_event().await;
        assert!(e2.is_none(), "stream should end on rate limit >98%");
        assert!(stream.rate_limit_exceeded);
    }

    #[tokio::test]
    async fn event_stream_rate_limit_below_threshold() {
        let ndjson = [
            r#"{"type":"system","subtype":"init","session_id":"sess-rl2"}"#,
            r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed_warning","utilization":0.75}}"#,
            r#"{"type":"assistant","content":[{"type":"text","text":"still going"}]}"#,
            r#"{"type":"result","session_id":"sess-rl2","subtype":"success","is_error":false,"num_turns":1,"duration_ms":100}"#,
        ]
        .join("\n");

        let mut stream = event_stream_from_bytes(ndjson.as_bytes());

        let e1 = stream.next_event().await;
        assert!(matches!(e1, Some(SessionEvent::TextDelta { ref text }) if text == "still going"));

        assert!(!stream.rate_limit_exceeded);
    }

    #[tokio::test]
    async fn event_stream_error_result_emits_error_event() {
        let ndjson = [
            r#"{"type":"system","subtype":"init","session_id":"sess-err"}"#,
            r#"{"type":"result","session_id":"sess-err","subtype":"error_during_execution","is_error":true,"num_turns":1,"duration_ms":500,"result":"something broke"}"#,
        ]
        .join("\n");

        let mut stream = event_stream_from_bytes(ndjson.as_bytes());

        let e1 = stream.next_event().await;
        assert!(
            matches!(e1, Some(SessionEvent::Error { ref message }) if message == "something broke")
        );

        let e2 = stream.next_event().await;
        assert!(e2.is_none());
    }

    #[tokio::test]
    async fn event_stream_multi_block_assistant() {
        let ndjson = [
            r#"{"type":"system","subtype":"init","session_id":"sess-mb"}"#,
            r#"{"type":"assistant","content":[{"type":"text","text":"first"},{"type":"text","text":"second"}]}"#,
            r#"{"type":"result","session_id":"sess-mb","subtype":"success","is_error":false,"num_turns":1,"duration_ms":100}"#,
        ]
        .join("\n");

        let mut stream = event_stream_from_bytes(ndjson.as_bytes());

        let e1 = stream.next_event().await;
        assert!(matches!(e1, Some(SessionEvent::TextDelta { ref text }) if text == "first"));

        let e2 = stream.next_event().await;
        assert!(matches!(e2, Some(SessionEvent::TextDelta { ref text }) if text == "second"));

        let e3 = stream.next_event().await;
        assert!(e3.is_none());
    }

    // -- helpers --

    /// Create an empty `ChildStdout` for synchronous tests that only exercise
    /// `map_content_blocks` (never actually read from the stream).
    fn make_empty_stdout() -> ChildStdout {
        // WHY: Spawn a trivial process to get a ChildStdout handle. The process
        // exits immediately so the stream will return EOF if read.
        let mut child = tokio::process::Command::new("true")
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("'true' command should be available");
        child.stdout.take().expect("stdout should be piped")
    }

    /// Create an `EventStream` backed by in-memory bytes instead of a real subprocess.
    fn event_stream_from_bytes(data: &[u8]) -> EventStream {
        // WHY: We need a ChildStdout for the type system but want to feed known
        // data. Use a shell process that echoes our test data.
        let escaped = String::from_utf8_lossy(data).replace('\'', "'\\''");
        let mut child = tokio::process::Command::new("sh")
            .args(["-c", &format!("printf '%s' '{escaped}'")])
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("sh command should be available");
        let stdout = child.stdout.take().expect("stdout should be piped");
        // WHY: Leak the child handle. It exits immediately after printf; the OS
        // reaps it. We only need the stdout pipe.
        std::mem::forget(child);
        EventStream::new(stdout)
    }
}
