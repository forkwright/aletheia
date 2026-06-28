//! Output truncation for daemon brief mode.

use std::process::ExitStatus;

use sha2::{Digest, Sha256};

use super::DaemonOutputMode;

/// Default lines to keep from tool output head in brief mode.
const DEFAULT_BRIEF_HEAD_LINES: usize = 5;
/// Default lines from the tail of tool output in brief mode.
const DEFAULT_BRIEF_TAIL_LINES: usize = 3;
const REDACTED_PATH: &str = "[PATH REDACTED]";
const PATH_MARKERS: &[&str] = &["/home/", "/Users/", "/tmp/", "/private/", "C:\\Users\\"];

#[derive(Debug, Clone, PartialEq, Eq)]
struct OutputSummary {
    bytes: usize,
    lines: usize,
    sha256: String,
}

impl OutputSummary {
    fn from_bytes(bytes: &[u8]) -> Self {
        Self {
            bytes: bytes.len(),
            lines: line_count(bytes),
            sha256: sha256_hex(bytes),
        }
    }

    fn format(&self, label: &str, excerpt: Option<&str>) -> String {
        let mut output = format!(
            "{label}: bytes={} lines={} sha256={}",
            self.bytes, self.lines, self.sha256
        );
        if let Some(excerpt) = excerpt
            && !excerpt.is_empty()
        {
            output.push_str("; redacted_excerpt:\n");
            output.push_str(excerpt);
        }
        output
    }
}

/// Return a stable, non-reversible command context for errors and tracing.
#[must_use]
pub(crate) fn command_context(command: &str) -> String {
    OutputSummary::from_bytes(command.as_bytes()).format("command summary", None)
}

/// Redact sensitive values and private local path segments from task text.
#[must_use]
pub(crate) fn redact_task_text(value: &str) -> String {
    redact_private_paths(&koina::redact::redact_sensitive(value))
}

/// Format task output for logging or persistence under the selected policy.
#[must_use]
pub(crate) fn safe_output_for_mode(
    output: &str,
    mode: DaemonOutputMode,
    behavior: &taxis::config::DaemonBehaviorConfig,
) -> String {
    match mode {
        DaemonOutputMode::Summary => {
            OutputSummary::from_bytes(output.as_bytes()).format("output summary", None)
        }
        DaemonOutputMode::Brief => {
            let redacted = redact_task_text(output);
            let excerpt = truncate_output(
                &redacted,
                Some(behavior.runner_output_brief_head_lines),
                Some(behavior.runner_output_brief_tail_lines),
            );
            OutputSummary::from_bytes(output.as_bytes()).format("output summary", Some(&excerpt))
        }
        DaemonOutputMode::Full => redact_task_text(output),
    }
}

/// Format process stdout/stderr for a command failure without exposing content.
#[must_use]
pub(crate) fn process_output_report(status: ExitStatus, stdout: &[u8], stderr: &[u8]) -> String {
    format!(
        "process output summary: exit_status={status}; stdout_bytes={} stdout_lines={} stdout_sha256={}; stderr_bytes={} stderr_lines={} stderr_sha256={}; output_sha256={}",
        stdout.len(),
        line_count(stdout),
        sha256_hex(stdout),
        stderr.len(),
        line_count(stderr),
        sha256_hex(stderr),
        process_digest(stdout, stderr)
    )
}

/// Truncate output for brief mode.
///
/// Keeps the first `head_lines` and last `tail_lines`, inserting
/// a `... (N lines omitted)` marker in between.
///
/// Pass `None` for either to use the defaults (5 head, 3 tail)
/// from [`DaemonBehaviorConfig`].
///
/// # Complexity
///
/// O(n) where n is the number of lines in the output.
#[expect(
    clippy::indexing_slicing,
    reason = "bounds checked: total > HEAD + TAIL before slicing"
)]
pub(crate) fn truncate_output(
    output: &str,
    head_lines: Option<usize>,
    tail_lines: Option<usize>,
) -> String {
    let brief_head = head_lines.unwrap_or(DEFAULT_BRIEF_HEAD_LINES);
    let brief_tail = tail_lines.unwrap_or(DEFAULT_BRIEF_TAIL_LINES);
    let lines: Vec<&str> = output.lines().collect();
    let total = lines.len();

    if total <= brief_head + brief_tail {
        return output.to_owned();
    }

    // kanon:ignore RUST/indexing-slicing — bounds checked: total > brief_head + brief_tail before slicing
    // kanon:ignore RUST/string-slice — slicing Vec<&str> (not String); bounds checked above
    let head = &lines[..brief_head];
    // kanon:ignore RUST/indexing-slicing — bounds checked: total > brief_head + brief_tail before slicing
    // kanon:ignore RUST/string-slice — slicing Vec<&str> (not String); bounds checked above
    let tail = &lines[total - brief_tail..];
    let omitted = total - brief_head - brief_tail;

    format!(
        "{}\n... ({omitted} lines omitted)\n{}",
        head.join("\n"),
        tail.join("\n")
    )
}

fn line_count(bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }

    let segments = bytes.split(|byte| *byte == b'\n').count();
    if bytes.ends_with(b"\n") {
        segments.saturating_sub(1)
    } else {
        segments
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    hex_lower(&digest)
}

fn process_digest(stdout: &[u8], stderr: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"stdout\0");
    hasher.update(stdout);
    hasher.update(b"\0stderr\0");
    hasher.update(stderr);
    let digest = hasher.finalize();
    hex_lower(&digest)
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        if let Some(high) = HEX.get(usize::from(byte >> 4)) {
            output.push(char::from(*high));
        }
        if let Some(low) = HEX.get(usize::from(byte & 0x0f)) {
            output.push(char::from(*low));
        }
    }
    output
}

fn redact_private_paths(value: &str) -> String {
    let mut redacted = value.to_owned();
    for marker in PATH_MARKERS {
        redacted = redact_marker_paths(&redacted, marker);
    }
    redacted
}

fn redact_marker_paths(value: &str, marker: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut rest = value;

    while let Some(start) = rest.find(marker) {
        let Some(prefix) = rest.get(..start) else {
            break;
        };
        output.push_str(prefix);
        output.push_str(REDACTED_PATH);

        let Some(candidate) = rest.get(start..) else {
            break;
        };
        let end = candidate
            .char_indices()
            .find_map(|(idx, ch)| path_terminator(ch).then_some(idx))
            .unwrap_or(candidate.len());
        let Some(next_rest) = candidate.get(end..) else {
            break;
        };
        rest = next_rest;
    }

    output.push_str(rest);
    output
}

fn path_terminator(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            '"' | '\'' | '`' | ')' | ']' | '}' | '<' | '>' | ',' | ';'
        )
}
