//! Output truncation for daemon brief mode.

/// Maximum lines to keep FROM tool output in brief mode (head + tail).
const BRIEF_HEAD_LINES: usize = 5;
/// Maximum lines FROM the tail of tool output in brief mode.
const BRIEF_TAIL_LINES: usize = 3;

/// Truncate output for brief mode.
///
/// Keeps the first `BRIEF_HEAD_LINES` and last `BRIEF_TAIL_LINES`, inserting
/// a `... (N lines omitted)` marker in between.
///
/// # Complexity
///
/// O(n) where n is the number of lines in the output.
#[expect(
    clippy::indexing_slicing,
    reason = "bounds checked: total > HEAD + TAIL before slicing"
)]
pub(crate) fn truncate_output(output: &str) -> String {
    let lines: Vec<&str> = output.lines().collect();
    let total = lines.len();

    if total <= BRIEF_HEAD_LINES + BRIEF_TAIL_LINES {
        return output.to_owned();
    }

    let head = &lines[..BRIEF_HEAD_LINES];
    let tail = &lines[total - BRIEF_TAIL_LINES..];
    let omitted = total - BRIEF_HEAD_LINES - BRIEF_TAIL_LINES;

    format!(
        "{}\n... ({omitted} lines omitted)\n{}",
        head.join("\n"),
        tail.join("\n")
    )
}
