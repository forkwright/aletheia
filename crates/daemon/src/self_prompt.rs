//! Self-prompting: daemon-initiated follow-up actions from prosoche checks.
//!
//! WHY: a nous should initiate work proactively, not just respond to user
//! messages. When a prosoche check identifies something needing attention, the
//! daemon extracts the follow-up and sends a self-prompt via the bridge. Rate
//! limiting prevents runaway loops.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

/// Session key used for all self-prompt dispatches.
///
/// WHY: separate session key prevents self-prompts from interfering with user
/// sessions. Users can check `daemon:self-prompt` when they want to review
/// autonomous actions.
pub const SELF_PROMPT_SESSION_KEY: &str = "daemon:self-prompt";

/// Minimal issue shape used to seed autonomous follow-up work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenIssue {
    /// Issue number in the tracker.
    pub number: u64,
    /// Issue title.
    pub title: String,
    /// Issue body, if the tracker provided one.
    #[serde(default)]
    pub body: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawIssue {
    number: u64,
    title: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    state: Option<String>,
}

/// Prompt task generated from an open issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuePromptTask {
    /// Stable daemon task id.
    // kanon:ignore RUST/primitive-for-domain-id — IssuePromptTask::id is a synthetic daemon task identifier derived from an issue number, not a domain entity ID
    pub id: String,
    /// Human-readable task name.
    pub name: String,
    /// Prompt sent to the nous.
    pub prompt: String,
}

/// Parse tracker JSON and retain only open issues.
///
/// The accepted shape is a JSON array of objects with `number`, `title`,
/// optional `body`, and optional `state`; missing `state` is treated as open
/// so tests and simple local snapshots can use a minimal issue shape.
pub fn parse_open_issues_json(input: &str) -> Result<Vec<OpenIssue>, serde_json::Error> {
    let issues: Vec<RawIssue> = serde_json::from_str(input)?;
    Ok(issues
        .into_iter()
        .filter(|issue| issue.state.as_deref().is_none_or(|state| state == "open"))
        .map(|issue| OpenIssue {
            number: issue.number,
            title: issue.title,
            body: issue.body.unwrap_or_default(),
        })
        .collect())
}

/// Generate a recurring self-prompt task from the lowest-numbered open issue.
#[must_use]
pub fn prompt_task_from_top_open_issue(issues: &[OpenIssue]) -> Option<IssuePromptTask> {
    let issue = issues.iter().min_by_key(|issue| issue.number)?;
    Some(IssuePromptTask {
        id: format!("issue-self-prompt-{}", issue.number),
        name: format!("Issue #{} self-prompt", issue.number),
        prompt: format_issue_prompt(issue),
    })
}

fn format_issue_prompt(issue: &OpenIssue) -> String {
    let mut prompt = format!(
        "Work the top open issue.\n\nIssue #{}: {}\n\n",
        issue.number, issue.title
    );
    if !issue.body.trim().is_empty() {
        prompt.push_str(issue.body.trim());
        prompt.push_str("\n\n");
    }
    prompt.push_str("Produce a concrete implementation plan and take the next useful action.");
    prompt
}

/// Configuration for self-prompting behavior.
///
/// WHY: self-prompting must be opt-in and rate-limited. Without explicit
/// enablement, the daemon never sends itself follow-up prompts. Without rate
/// limits, a misidentified attention item could trigger an unbounded loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelfPromptConfig {
    /// Whether self-prompting is enabled.
    #[serde(default)]
    pub enabled: bool,

    /// Maximum self-prompts per hour per agent.
    ///
    /// WHY: rate limit prevents runaway loops. A prosoche check that always
    /// produces a `## Follow-up` section would otherwise generate unbounded work.
    #[serde(default = "default_max_per_hour")]
    pub max_per_hour: u32,
}

fn default_max_per_hour() -> u32 {
    1
}

impl Default for SelfPromptConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_per_hour: default_max_per_hour(),
        }
    }
}

/// Tracks self-prompt counts per agent for rate limiting.
///
/// Uses a sliding window: timestamps older than 1 hour are pruned on each check.
#[derive(Debug)]
pub(crate) struct SelfPromptLimiter {
    /// Per-agent timestamps of dispatched self-prompts.
    windows: HashMap<String, Vec<jiff::Timestamp>>,
    /// Maximum allowed per hour (from config).
    max_per_hour: u32,
}

impl SelfPromptLimiter {
    /// Create a new limiter with the given rate cap.
    pub(crate) fn new(max_per_hour: u32) -> Self {
        Self {
            windows: HashMap::new(),
            max_per_hour,
        }
    }

    /// Check whether a self-prompt is allowed for the given agent right now.
    ///
    /// Prunes expired entries as a side effect.
    pub(crate) fn is_allowed(&mut self, nous_id: &str) -> bool {
        // kanon:ignore RUST/no-result-unwrap-or-default — subtracting 1h from now cannot underflow; default is defensive-only
        let cutoff = jiff::Timestamp::now()
            .checked_sub(jiff::SignedDuration::from_hours(1))
            .unwrap_or_default();

        let timestamps = self.windows.entry(nous_id.to_owned()).or_default();

        // Prune entries older than 1 hour.
        timestamps.retain(|ts| *ts > cutoff);

        // WHY: saturate to u32::MAX so an absurdly full window trips the rate
        // limit; the realistic bound is max_per_hour << u32::MAX.
        let count = u32::try_from(timestamps.len()).unwrap_or(u32::MAX);
        count < self.max_per_hour
    }

    /// Record that a self-prompt was dispatched for the given agent.
    pub(crate) fn record(&mut self, nous_id: &str) {
        self.windows
            .entry(nous_id.to_owned())
            .or_default()
            .push(jiff::Timestamp::now());
    }

    /// Current count of self-prompts in the window for the given agent.
    #[cfg(test)]
    pub(crate) fn count(&mut self, nous_id: &str) -> usize {
        let cutoff = jiff::Timestamp::now()
            .checked_sub(jiff::SignedDuration::from_hours(1))
            .unwrap_or_default();

        let timestamps = self.windows.entry(nous_id.to_owned()).or_default();
        timestamps.retain(|ts| *ts > cutoff);
        timestamps.len()
    }
}

/// Extract a self-prompt from prosoche or task output.
///
/// Looks for a `## Follow-up` markdown section in the output text. Everything
/// after that heading (until the next `##` heading or end of string) becomes the
/// self-prompt content.
///
/// WHY: structured extraction means the agent controls what becomes a
/// self-prompt. Free-form text won't accidentally trigger follow-ups.
pub(crate) fn extract_follow_up(output: &str) -> Option<String> {
    // Find the `## Follow-up` heading (case-insensitive for the word "follow-up").
    let lower = output.to_lowercase();
    let marker = "## follow-up";

    let start_idx = lower.find(marker)?;
    let content_start = start_idx + marker.len();

    // Skip the rest of the heading line.
    // WHY: `start_idx + marker.len() <= output.len()` because `find` returns the
    // byte index of the marker and `output` has the same byte length as `lower`.
    // `.get()` returns `None` only if `content_start > output.len()`, which
    // cannot happen here, but use it anyway to satisfy clippy::string_slice.
    let after_heading = output.get(content_start..)?;
    let line_end = after_heading.find('\n').unwrap_or(after_heading.len());
    let body_start = content_start + line_end;

    // `body_start = content_start + line_end <= content_start + after_heading.len() == output.len()`
    let body = output.get(body_start..)?;
    if body.is_empty() {
        return None;
    }

    // Terminate at the next `##` heading or end of string.
    let end = body.find("\n## ").unwrap_or(body.len());

    // `end <= body.len()` by construction (find returns Some(pos<len) or unwrap_or(body.len())).
    let content = body.get(..end)?.trim();

    if content.is_empty() {
        None
    } else {
        Some(content.to_owned())
    }
}

/// Execute a self-prompt: send the extracted follow-up to the nous via the bridge.
///
/// Legacy entry point used by tests; uses a fresh cancellation token. The runner
/// uses [`execute_self_prompt_with_cancel`] to propagate task cancellation.
#[cfg(test)]
#[tracing::instrument(skip_all)]
pub(crate) async fn execute_self_prompt(
    nous_id: &str,
    prompt: &str,
    bridge: Option<&dyn crate::bridge::DaemonBridge>,
) -> crate::error::Result<crate::runner::ExecutionResult> {
    execute_self_prompt_with_cancel(nous_id, prompt, bridge, CancellationToken::new()).await
}

/// Execute a self-prompt with a cancellation token propagated to the bridge.
#[tracing::instrument(skip_all)]
pub(crate) async fn execute_self_prompt_with_cancel(
    nous_id: &str,
    prompt: &str,
    bridge: Option<&dyn crate::bridge::DaemonBridge>,
    cancel: CancellationToken,
) -> crate::error::Result<crate::runner::ExecutionResult> {
    let Some(bridge) = bridge else {
        return Ok(crate::runner::ExecutionResult {
            success: false,
            output: Some("no bridge configured".to_owned()),
        });
    };

    tracing::info!(
        nous_id = %nous_id,
        prompt_len = prompt.len(),
        "dispatching self-prompt"
    );

    match bridge
        .send_prompt_with_cancel(nous_id, SELF_PROMPT_SESSION_KEY, prompt, cancel)
        .await
    {
        Ok(result) => {
            tracing::info!(
                nous_id = %nous_id,
                success = result.success,
                "self-prompt dispatch succeeded"
            );
            Ok(crate::runner::ExecutionResult {
                success: true,
                output: Some("self-prompt dispatched".to_owned()),
            })
        }
        Err(e) => {
            tracing::warn!(
                nous_id = %nous_id,
                error = %e,
                "self-prompt dispatch failed"
            );
            Ok(crate::runner::ExecutionResult {
                success: false,
                output: Some(format!("self-prompt dispatch failed: {e}")),
            })
        }
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    // -- SelfPromptConfig tests --

    #[test]
    fn default_config_disabled() {
        let config = SelfPromptConfig::default();
        assert!(
            !config.enabled,
            "self-prompting must be disabled by default"
        );
        assert_eq!(config.max_per_hour, 1);
    }

    #[test]
    fn config_serialization_roundtrip() {
        let config = SelfPromptConfig {
            enabled: true,
            max_per_hour: 3,
        };
        let json = serde_json::to_string(&config).expect("serialize");
        let back: SelfPromptConfig = serde_json::from_str(&json).expect("deserialize");
        assert!(back.enabled);
        assert_eq!(back.max_per_hour, 3);
    }

    #[test]
    fn config_deserialize_defaults() {
        let json = "{}";
        let config: SelfPromptConfig = serde_json::from_str(json).expect("deserialize");
        assert!(!config.enabled);
        assert_eq!(config.max_per_hour, 1);
    }

    #[test]
    fn parses_open_issues_from_tracker_json() {
        let json = r#"[
            {"number": 2, "title": "closed item", "body": "done", "state": "closed"},
            {"number": 1, "title": "wire issue prompts", "body": "Use title and body.", "state": "open"}
        ]"#;
        let issues = parse_open_issues_json(json).expect("parse issues");
        assert_eq!(issues.len(), 1);
        let Some(issue) = issues.first() else {
            panic!("expected one open issue");
        };
        assert_eq!(issue.number, 1);
        assert_eq!(issue.title, "wire issue prompts");
    }

    #[test]
    fn top_open_issue_generates_prompt_task() {
        let issues = vec![
            OpenIssue {
                number: 4,
                title: "later task".to_owned(),
                body: String::new(),
            },
            OpenIssue {
                number: 1,
                title: "Generate issue prompt".to_owned(),
                body: "Use the issue body as context.".to_owned(),
            },
        ];

        let task = prompt_task_from_top_open_issue(&issues).expect("task");
        assert_eq!(task.id, "issue-self-prompt-1");
        assert!(task.prompt.contains("Issue #1: Generate issue prompt"));
        assert!(task.prompt.contains("Use the issue body as context."));
    }

    // -- SelfPromptLimiter tests --

    #[test]
    fn limiter_allows_first_prompt() {
        let mut limiter = SelfPromptLimiter::new(1);
        assert!(
            limiter.is_allowed("test-nous"),
            "first prompt should be allowed"
        );
    }

    #[test]
    fn limiter_blocks_after_max() {
        let mut limiter = SelfPromptLimiter::new(1);
        limiter.record("test-nous");
        assert!(
            !limiter.is_allowed("test-nous"),
            "second prompt within window should be blocked"
        );
    }

    #[test]
    fn limiter_allows_different_agents() {
        let mut limiter = SelfPromptLimiter::new(1);
        limiter.record("nous-a");
        assert!(
            limiter.is_allowed("nous-b"),
            "different agent should be independent"
        );
    }

    #[test]
    fn limiter_tracks_count() {
        let mut limiter = SelfPromptLimiter::new(5);
        limiter.record("test-nous");
        limiter.record("test-nous");
        limiter.record("test-nous");
        assert_eq!(limiter.count("test-nous"), 3);
    }

    #[test]
    fn limiter_higher_max_allows_multiple() {
        let mut limiter = SelfPromptLimiter::new(3);
        limiter.record("test-nous");
        limiter.record("test-nous");
        assert!(
            limiter.is_allowed("test-nous"),
            "should allow up to max_per_hour"
        );
        limiter.record("test-nous");
        assert!(
            !limiter.is_allowed("test-nous"),
            "should block at max_per_hour"
        );
    }

    // -- extract_follow_up tests --

    #[test]
    fn extract_follow_up_basic() {
        let output = "Some output\n\n## Follow-up\n\nCheck disk space on /data.\n";
        let follow_up = extract_follow_up(output).expect("should extract");
        assert_eq!(follow_up, "Check disk space on /data.");
    }

    #[test]
    fn extract_follow_up_case_insensitive() {
        let output = "Results:\n## follow-up\nReview memory usage trends.\n";
        let follow_up = extract_follow_up(output).expect("should extract");
        assert_eq!(follow_up, "Review memory usage trends.");
    }

    #[test]
    fn extract_follow_up_stops_at_next_heading() {
        let output = concat!(
            "## Summary\nAll good.\n",
            "## Follow-up\nInvestigate slow query.\n",
            "## Notes\nExtra info.\n",
        );
        let follow_up = extract_follow_up(output).expect("should extract");
        assert_eq!(follow_up, "Investigate slow query.");
    }

    #[test]
    fn extract_follow_up_multiline() {
        let output = concat!(
            "## Follow-up\n",
            "1. Check disk usage on /data\n",
            "2. Review database size growth\n",
            "3. Prune old trace files\n",
        );
        let follow_up = extract_follow_up(output).expect("should extract");
        assert!(follow_up.contains("Check disk usage"));
        assert!(follow_up.contains("Prune old trace files"));
    }

    #[test]
    fn extract_follow_up_none_when_missing() {
        let output = "Everything is fine.\n## Summary\nNo issues.\n";
        assert!(extract_follow_up(output).is_none());
    }

    #[test]
    fn extract_follow_up_none_when_empty_body() {
        let output = "## Follow-up\n\n## Next Section\nStuff.\n";
        assert!(
            extract_follow_up(output).is_none(),
            "empty follow-up body should return None"
        );
    }

    #[test]
    fn extract_follow_up_none_when_heading_only() {
        let output = "## Follow-up";
        assert!(
            extract_follow_up(output).is_none(),
            "heading-only should return None"
        );
    }

    // -- execute_self_prompt tests --

    #[tokio::test]
    async fn execute_without_bridge_returns_failure() {
        let result = execute_self_prompt("test-nous", "do something", None)
            .await
            .expect("should not error");
        assert!(!result.success);
        assert!(result.output.expect("has output").contains("no bridge"));
    }

    #[tokio::test]
    async fn execute_with_noop_bridge_dispatches() {
        let bridge = crate::bridge::NoopBridge;
        let result = execute_self_prompt("test-nous", "investigate disk", Some(&bridge))
            .await
            .expect("should not error");
        // NOTE: NoopBridge returns success=false, but the dispatch itself succeeds.
        assert!(result.success);
        assert!(
            result
                .output
                .expect("has output")
                .contains("self-prompt dispatched")
        );
    }

    // -- Session key constant test --

    #[test]
    fn session_key_is_daemon_prefixed() {
        assert!(
            SELF_PROMPT_SESSION_KEY.starts_with("daemon:"),
            "self-prompt session key must use daemon: prefix"
        );
    }
}
