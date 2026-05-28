//! Output truncation for daemon brief mode.

/// Default lines to keep from tool output head in brief mode.
const DEFAULT_BRIEF_HEAD_LINES: usize = 5;
/// Default lines from the tail of tool output in brief mode.
const DEFAULT_BRIEF_TAIL_LINES: usize = 3;

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
