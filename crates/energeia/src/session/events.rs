// WHY: Processes the SessionEvent stream from a SessionHandle, accumulating
// cost and turn metrics while detecting timeouts and PR URLs.
// Separated from the manager so event logic is testable without the full
// resume loop.

use std::time::{Duration, Instant};

use crate::engine::{SessionEvent, SessionHandle};

// ---------------------------------------------------------------------------
// EventAccumulator
// ---------------------------------------------------------------------------

/// Accumulated metrics from processing a session's event stream.
#[derive(Debug, Clone)]
pub(crate) struct EventAccumulator {
    /// Total cost in USD observed during this stream.
    pub cost_usd: f64,
    /// Number of turns completed.
    pub num_turns: u32,
    /// Collected text fragments from assistant output.
    pub text_fragments: Vec<String>,
}

impl EventAccumulator {
    pub(crate) fn new() -> Self {
        Self {
            cost_usd: 0.0,
            num_turns: 0,
            text_fragments: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// StreamOutcome — how the event stream terminated
// ---------------------------------------------------------------------------

/// How the session's event stream terminated.
#[derive(Debug)]
pub(crate) enum StreamOutcome {
    /// Stream ended normally (all events consumed).
    Complete(EventAccumulator),
    /// No events received within the configured timeout.
    Timeout {
        accumulator: EventAccumulator,
        elapsed: Duration,
    },
    /// An error event was received from the session.
    Error {
        accumulator: EventAccumulator,
        message: String,
    },
}

// ---------------------------------------------------------------------------
// process_events
// ---------------------------------------------------------------------------

/// Drain the event stream from a session handle, accumulating metrics.
///
/// Stops early on timeout or error events. Returns a [`StreamOutcome`]
/// describing how the stream terminated.
///
/// `idle_timeout` controls how long we wait for the next event before treating
/// the session as stalled. Pass `None` to disable timeout detection.
pub(crate) async fn process_events(
    handle: &mut Box<dyn SessionHandle>,
    idle_timeout: Option<Duration>,
) -> StreamOutcome {
    let mut acc = EventAccumulator::new();
    let mut last_event_time = Instant::now();

    loop {
        // WHY: Check idle timeout before awaiting the next event. If we've
        // already exceeded the timeout, don't wait for another event.
        if let Some(timeout) = idle_timeout {
            let elapsed = last_event_time.elapsed();
            if elapsed >= timeout {
                return StreamOutcome::Timeout {
                    accumulator: acc,
                    elapsed,
                };
            }
        }

        let event = if let Some(timeout) = idle_timeout {
            let remaining = timeout.saturating_sub(last_event_time.elapsed());
            match tokio::time::timeout(remaining, handle.next_event()).await {
                Ok(event) => event,
                Err(_) => {
                    return StreamOutcome::Timeout {
                        accumulator: acc,
                        elapsed: last_event_time.elapsed(),
                    };
                }
            }
        } else {
            handle.next_event().await
        };

        let Some(event) = event else {
            // Stream exhausted — session complete.
            return StreamOutcome::Complete(acc);
        };

        last_event_time = Instant::now();

        match event {
            SessionEvent::TextDelta { text } => {
                acc.text_fragments.push(text);
            }
            SessionEvent::ToolUse { .. } | SessionEvent::ToolResult { .. } => {
                // NOTE: Tool events are observed but don't carry cost data in
                // the current `SessionEvent` model. Cost comes from `SessionResult`.
            }
            SessionEvent::TurnComplete { turn } => {
                acc.num_turns = turn;
            }
            SessionEvent::Error { message } => {
                return StreamOutcome::Error {
                    accumulator: acc,
                    message,
                };
            }
        }
    }
}

// ---------------------------------------------------------------------------
// PR URL extraction
// ---------------------------------------------------------------------------

/// Extract a GitHub pull request URL from text.
///
/// Matches `https://github.com/{owner}/{repo}/pull/{number}` patterns.
/// Returns the first match found.
pub(crate) fn extract_pr_url(text: &str) -> Option<&str> {
    // WHY: Simple substring search avoids a regex dependency. PR URLs have a
    // predictable structure and we only need the first match.
    const PREFIX: &str = "https://github.com/";

    let mut search_from = 0;

    while let Some(start) = text.get(search_from..)?.find(PREFIX) {
        let abs_start = search_from + start;
        let rest = text.get(abs_start..)?;

        // NOTE: Find the end of the URL (whitespace, quote, paren, or end of string).
        let end = rest
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ')' | '>' | ']'))
            .unwrap_or(rest.len());

        let candidate = rest.get(..end)?;

        // NOTE: Validate it contains /pull/ followed by digits.
        if let Some(pull_pos) = candidate.find("/pull/") {
            let after_pull = candidate.get(pull_pos + "/pull/".len()..)?;
            if !after_pull.is_empty() && after_pull.chars().all(|c| c.is_ascii_digit()) {
                return Some(candidate);
            }
        }

        // WHY: Advance past "github.com/" to avoid re-matching the same prefix.
        search_from = abs_start + "github.com/".len();
    }

    None
}

/// The utilization threshold above which sessions should be aborted.
///
/// WHY: At >98% utilization the API will start rejecting requests imminently.
/// Aborting early avoids wasting turns on requests that will fail.
///
/// NOTE: Will be consumed by the session manager once `SessionEvent` gains a
/// rate-limit variant (pending Agent SDK integration).
// NOTE: Will be consumed by the session manager once `SessionEvent` gains a
// rate-limit variant (pending Agent SDK integration). Used in tests only for now.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "constant defined for use once SessionEvent gains rate-limit events"
    )
)]
pub(crate) const RATE_LIMIT_ABORT_THRESHOLD: f64 = 0.98;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::engine::DispatchEngine;
    use crate::engine::{AgentOptions, SessionResult, SessionSpec};
    use crate::http::mock::{MockEngine, MockOutcome};

    // ---- PR URL extraction ----

    #[test]
    fn extract_pr_url_from_text() {
        let text = "Created PR: https://github.com/acme/repo/pull/42 for review";
        assert_eq!(
            extract_pr_url(text),
            Some("https://github.com/acme/repo/pull/42")
        );
    }

    #[test]
    fn extract_pr_url_at_end_of_string() {
        let text = "PR: https://github.com/acme/repo/pull/123";
        assert_eq!(
            extract_pr_url(text),
            Some("https://github.com/acme/repo/pull/123")
        );
    }

    #[test]
    fn extract_pr_url_with_org_slash() {
        let text = "https://github.com/my-org/my-repo/pull/999";
        assert_eq!(
            extract_pr_url(text),
            Some("https://github.com/my-org/my-repo/pull/999")
        );
    }

    #[test]
    fn extract_pr_url_ignores_non_pull_github_urls() {
        let text = "See https://github.com/acme/repo/issues/42";
        assert!(extract_pr_url(text).is_none());
    }

    #[test]
    fn extract_pr_url_ignores_pull_without_number() {
        let text = "https://github.com/acme/repo/pull/";
        assert!(extract_pr_url(text).is_none());
    }

    #[test]
    fn extract_pr_url_returns_none_for_no_url() {
        assert!(extract_pr_url("no urls here").is_none());
    }

    #[test]
    fn extract_pr_url_in_markdown_link() {
        let text = "[PR](https://github.com/acme/repo/pull/7)";
        assert_eq!(
            extract_pr_url(text),
            Some("https://github.com/acme/repo/pull/7")
        );
    }

    #[test]
    fn extract_pr_url_in_quotes() {
        let text = r#"url: "https://github.com/acme/repo/pull/55""#;
        assert_eq!(
            extract_pr_url(text),
            Some("https://github.com/acme/repo/pull/55")
        );
    }

    // ---- Rate limit threshold ----

    #[test]
    fn rate_limit_threshold_value() {
        assert!((RATE_LIMIT_ABORT_THRESHOLD - 0.98).abs() < f64::EPSILON);
    }

    // ---- Event processing ----

    fn make_result(session_id: &str, success: bool) -> SessionResult {
        SessionResult {
            session_id: session_id.to_owned(),
            cost_usd: 0.10,
            num_turns: 5,
            duration_ms: 2000,
            success,
            result_text: Some("done".to_owned()),
            model: Some("claude-3-5-sonnet".to_owned()),
        }
    }

    fn make_spec() -> SessionSpec {
        SessionSpec {
            prompt: "test".to_owned(),
            system_prompt: None,
            cwd: None,
        }
    }

    #[tokio::test]
    async fn process_events_collects_text_and_turns() {
        let engine = MockEngine::new(vec![MockOutcome::Success {
            events: vec![
                SessionEvent::TextDelta {
                    text: "hello ".to_owned(),
                },
                SessionEvent::TurnComplete { turn: 1 },
                SessionEvent::TextDelta {
                    text: "world".to_owned(),
                },
                SessionEvent::TurnComplete { turn: 2 },
            ],
            result: make_result("sess-1", true),
        }]);

        let mut handle = engine
            .spawn_session(&make_spec(), &AgentOptions::new())
            .await
            .unwrap();

        let outcome = process_events(&mut handle, None).await;
        match outcome {
            StreamOutcome::Complete(acc) => {
                assert_eq!(acc.num_turns, 2);
                assert_eq!(acc.text_fragments.len(), 2);
                assert_eq!(
                    acc.text_fragments.first().map(String::as_str),
                    Some("hello ")
                );
                assert_eq!(acc.text_fragments.get(1).map(String::as_str), Some("world"));
            }
            other => panic!("expected Complete, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn process_events_returns_error_on_error_event() {
        let engine = MockEngine::new(vec![MockOutcome::Success {
            events: vec![
                SessionEvent::TurnComplete { turn: 1 },
                SessionEvent::Error {
                    message: "something broke".to_owned(),
                },
            ],
            result: make_result("sess-err", false),
        }]);

        let mut handle = engine
            .spawn_session(&make_spec(), &AgentOptions::new())
            .await
            .unwrap();

        let outcome = process_events(&mut handle, None).await;
        match outcome {
            StreamOutcome::Error {
                message,
                accumulator,
            } => {
                assert_eq!(message, "something broke");
                assert_eq!(accumulator.num_turns, 1);
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn process_events_completes_on_empty_stream() {
        let engine = MockEngine::new(vec![MockOutcome::Success {
            events: vec![],
            result: make_result("sess-timeout", true),
        }]);

        let mut handle = engine
            .spawn_session(&make_spec(), &AgentOptions::new())
            .await
            .unwrap();

        // NOTE: With an empty event stream, the mock returns None immediately,
        // so we get Complete rather than Timeout. This is correct behavior —
        // timeout only fires when events stop arriving mid-stream.
        let outcome = process_events(&mut handle, Some(Duration::from_millis(10))).await;
        assert!(matches!(outcome, StreamOutcome::Complete(_)));
    }
}
