//! Helper functions for tool metadata, argument extraction, and diff parsing.

use super::types::{OpsDiffEntry, ToolCategory, ToolMetadata, ToolRisk};

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

/// Categorize a tool name into a [`ToolCategory`] as an unverified compatibility fallback.
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

/// Build verified metadata from server-owned tool fields.
#[expect(
    clippy::too_many_arguments,
    reason = "metadata fields mirror the server DTO; grouping them would add an extra transient type"
)]
pub(crate) fn verified_tool_metadata(
    category: Option<&str>,
    reversibility: Option<&str>,
    approval: Option<&str>,
    requires_approval: bool,
    destructive: bool,
    source_plane: Option<String>,
    policy_state: Option<String>,
    unavailable_reason: Option<String>,
) -> ToolMetadata {
    let risk = risk_from_metadata(approval, reversibility, requires_approval, destructive);
    ToolMetadata {
        category: category
            .and_then(category_from_metadata)
            .unwrap_or(ToolCategory::Other),
        risk,
        reversibility: reversibility.map(str::to_owned),
        approval: approval.map(str::to_owned),
        requires_approval,
        destructive: destructive || risk.is_destructive(),
        source_plane,
        policy_state,
        unavailable_reason,
        verified: true,
    }
}

/// Build explicit unverified metadata from the legacy name heuristic.
pub(crate) fn unverified_tool_metadata(name: &str) -> ToolMetadata {
    let category = categorize_tool(name);
    let risk = fallback_risk(category);
    ToolMetadata {
        category,
        risk,
        reversibility: None,
        approval: None,
        requires_approval: risk.is_destructive(),
        destructive: risk.is_destructive(),
        source_plane: None,
        policy_state: None,
        unavailable_reason: None,
        verified: false,
    }
}

fn category_from_metadata(category: &str) -> Option<ToolCategory> {
    match category.to_ascii_lowercase().as_str() {
        "workspace" => Some(ToolCategory::Workspace),
        "memory" => Some(ToolCategory::Memory),
        "communication" => Some(ToolCategory::Communication),
        "planning" => Some(ToolCategory::Planning),
        "system" => Some(ToolCategory::System),
        "agent" => Some(ToolCategory::Agent),
        "research" => Some(ToolCategory::Research),
        "domain" => Some(ToolCategory::Domain),
        "server" => Some(ToolCategory::Server),
        "read" => Some(ToolCategory::Read),
        "write" => Some(ToolCategory::Write),
        "search" => Some(ToolCategory::Search),
        "exec" => Some(ToolCategory::Exec),
        "http" => Some(ToolCategory::Http),
        "other" => Some(ToolCategory::Other),
        _ => None,
    }
}

fn risk_from_metadata(
    approval: Option<&str>,
    reversibility: Option<&str>,
    requires_approval: bool,
    destructive: bool,
) -> ToolRisk {
    match approval {
        Some("mandatory") => return ToolRisk::Critical,
        Some("required") => return ToolRisk::High,
        Some("advisory") => return ToolRisk::Medium,
        Some("none") => return ToolRisk::Low,
        _ => {}
    }

    match reversibility {
        Some("irreversible") => ToolRisk::Critical,
        Some("partially_reversible") => ToolRisk::High,
        Some("reversible") => ToolRisk::Medium,
        Some("fully_reversible") => ToolRisk::Low,
        _ if requires_approval || destructive => ToolRisk::High,
        _ => ToolRisk::Medium,
    }
}

fn fallback_risk(category: ToolCategory) -> ToolRisk {
    match category {
        ToolCategory::Read | ToolCategory::Search => ToolRisk::Low,
        ToolCategory::Write | ToolCategory::Http => ToolRisk::High,
        ToolCategory::Exec => ToolRisk::Critical,
        _ => ToolRisk::Medium,
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
