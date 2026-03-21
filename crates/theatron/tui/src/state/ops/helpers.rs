//! Helper functions for tool categorization, argument extraction, and diff parsing.

use super::types::{OpsDiffEntry, ToolCategory};

/// Maximum length for the inline primary arg display.
const PRIMARY_ARG_MAX_LEN: usize = 40;

/// Maximum length for the inline error summary.
pub(super) const ERROR_MAX_LEN: usize = 80;

/// Fields to try, in priority order, when extracting the primary arg from tool input JSON.
const PRIMARY_ARG_KEYS: &[&str] = &[
    "file_path",
    "path",
    "command",
    "pattern",
    "query",
    "url",
    "glob",
];

/// Categorize a tool name into a [`ToolCategory`].
pub(crate) fn categorize_tool(name: &str) -> ToolCategory {
    let lower = name.to_lowercase();
    if lower.contains("read") || lower.contains("glob") || lower.contains("grep") {
        ToolCategory::Read
    } else if lower.contains("write")
        || lower.contains("edit")
        || lower.contains("patch")
        || lower.contains("notebook")
    {
        ToolCategory::Write
    } else if lower.contains("search") {
        ToolCategory::Search
    } else if lower.contains("bash") || lower.contains("exec") || lower.contains("shell") {
        ToolCategory::Exec
    } else if lower.contains("fetch") || lower.contains("http") || lower.contains("web_fetch") {
        ToolCategory::Http
    } else {
        ToolCategory::Other
    }
}

/// Extract the most informative argument from a tool's input JSON.
pub(crate) fn extract_primary_arg(json_str: &str, _tool_name: &str) -> Option<String> {
    let obj: serde_json::Value = serde_json::from_str(json_str).ok()?;
    let map = obj.as_object()?;

    for key in PRIMARY_ARG_KEYS {
        if let Some(val) = map.get(*key).and_then(|v| v.as_str())
            && !val.is_empty()
        {
            return Some(truncate_str(val, PRIMARY_ARG_MAX_LEN));
        }
    }
    None
}

/// Truncate a string to `max_len` chars, appending ellipsis if truncated.
pub(super) fn truncate_str(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len.saturating_sub(1)).collect();
        format!("{truncated}\u{2026}")
    }
}

/// Extract a one-line error summary from tool result text.
pub(crate) fn truncate_error(text: &str) -> String {
    let first_line = text.lines().next().unwrap_or(text);
    truncate_str(first_line, ERROR_MAX_LEN)
}

/// Try to parse a unified diff from a tool output string.
pub(crate) fn parse_diff_from_output(output: &str, tool_name: &str) -> Option<OpsDiffEntry> {
    let is_file_tool =
        tool_name.contains("write") || tool_name.contains("edit") || tool_name.contains("patch");
    if !is_file_tool {
        return None;
    }

    let mut additions = Vec::new();
    let mut deletions = Vec::new();
    let mut file_path = String::new();

    for line in output.lines() {
        if line.starts_with("--- ") || line.starts_with("+++ ") {
            let path = line.get(4..).unwrap_or("").trim().to_string();
            if !path.is_empty() && file_path.is_empty() {
                file_path = path;
            }
        } else if let Some(stripped) = line.strip_prefix('+') {
            if !stripped.is_empty() {
                additions.push(stripped.to_string());
            }
        } else if let Some(stripped) = line.strip_prefix('-')
            && !stripped.is_empty()
        {
            deletions.push(stripped.to_string());
        }
    }

    if additions.is_empty() && deletions.is_empty() {
        return None;
    }

    Some(OpsDiffEntry {
        file_path: if file_path.is_empty() {
            "unknown".to_string()
        } else {
            file_path
        },
        additions,
        deletions,
    })
}
