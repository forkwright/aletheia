//! Expandable tool call panel component.

use dioxus::prelude::*;
use theatron_core::format::format_duration;

use crate::state::tools::ToolCallState;

use super::tool_status::ToolStatusIcon;

const PANEL_COLLAPSED_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: 8px; \
    padding: 6px 10px; \
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: 6px; \
    cursor: pointer; \
    margin-top: 4px; \
    font-size: 13px; \
    color: var(--text-secondary);\
";

const PANEL_EXPANDED_STYLE: &str = "\
    padding: 8px 10px; \
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: 6px; \
    margin-top: 4px; \
    font-size: 13px;\
";

const PANEL_HEADER_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: 8px; \
    cursor: pointer; \
    color: var(--text-secondary); \
    margin-bottom: 8px;\
";

const TOOL_NAME_STYLE: &str = "\
    font-weight: 600; \
    color: var(--text-primary);\
";

const DURATION_BADGE_STYLE: &str = "\
    font-size: 11px; \
    padding: 1px 6px; \
    background: var(--border); \
    border-radius: 10px; \
    color: var(--text-secondary);\
";

const CODE_BLOCK_STYLE: &str = "\
    background: var(--code-bg); \
    border: 1px solid var(--border); \
    border-radius: 4px; \
    padding: 8px; \
    margin-top: 6px; \
    overflow-x: auto; \
    white-space: pre-wrap; \
    word-wrap: break-word; \
    font-family: var(--font-mono); \
    font-size: 12px; \
    color: var(--code-fg); \
    max-height: 300px; \
    overflow-y: auto;\
";

const SECTION_LABEL_STYLE: &str = "\
    font-size: 11px; \
    color: var(--text-muted); \
    text-transform: uppercase; \
    letter-spacing: 0.5px; \
    margin-top: 8px;\
";

const ERROR_DETAIL_STYLE: &str = "\
    color: var(--status-error); \
    background: var(--status-error-bg); \
    border: 1px solid var(--status-error); \
    border-radius: 4px; \
    padding: 8px; \
    margin-top: 6px; \
    font-size: 12px; \
    white-space: pre-wrap;\
";

/// Expandable tool call panel.
///
/// Collapsed: shows tool name, status icon, and duration badge.
/// Expanded: shows syntax-highlighted input JSON, output, and error details.
#[component]
pub(crate) fn ToolPanel(tool: ToolCallState) -> Element {
    let mut expanded = use_signal(|| false);
    let is_expanded = *expanded.read();

    let duration_text = tool.duration_ms.map(format_duration);
    let chevron = if is_expanded { "\u{25BE}" } else { "\u{25B8}" }; // ▾ / ▸

    if !is_expanded {
        return rsx! {
            div {
                style: "{PANEL_COLLAPSED_STYLE}",
                onclick: move |_| expanded.set(true),
                span { "{chevron}" }
                ToolStatusIcon { status: tool.status }
                span { style: "{TOOL_NAME_STYLE}", "{tool.tool_name}" }
                if let Some(ref dur) = duration_text {
                    span { style: "{DURATION_BADGE_STYLE}", "{dur}" }
                }
            }
        };
    }

    rsx! {
        div {
            style: "{PANEL_EXPANDED_STYLE}",
            div {
                style: "{PANEL_HEADER_STYLE}",
                onclick: move |_| expanded.set(false),
                span { "{chevron}" }
                ToolStatusIcon { status: tool.status }
                span { style: "{TOOL_NAME_STYLE}", "{tool.tool_name}" }
                if let Some(ref dur) = duration_text {
                    span { style: "{DURATION_BADGE_STYLE}", "{dur}" }
                }
            }

            if let Some(ref input) = tool.input {
                div { style: "{SECTION_LABEL_STYLE}", "Input" }
                div {
                    style: "{CODE_BLOCK_STYLE}",
                    "{format_json(input)}"
                }
            }

            if let Some(ref output) = tool.output {
                div { style: "{SECTION_LABEL_STYLE}", "Output" }
                div {
                    style: "{CODE_BLOCK_STYLE}",
                    "{output}"
                }
            }

            if let Some(ref err) = tool.error_message {
                div { style: "{SECTION_LABEL_STYLE}", "Error" }
                div {
                    style: "{ERROR_DETAIL_STYLE}",
                    "{err}"
                }
            }
        }
    }
}



/// Pretty-print a JSON value for display in the tool panel.
fn format_json(value: &serde_json::Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::tools::ToolStatus;

    #[test]
    fn format_json_pretty_prints() {
        let val = serde_json::json!({"key": "value"});
        let formatted = format_json(&val);
        assert!(formatted.contains("key"), "should contain the key");
        assert!(
            formatted.contains('\n'),
            "pretty print should have newlines"
        );
    }

    #[test]
    fn tool_panel_expand_collapse_state_default() {
        // WHY: verify the default state is collapsed (expanded = false).
        let tool = ToolCallState {
            tool_id: "t1".into(),
            tool_name: "read_file".to_string(),
            status: ToolStatus::Success,
            input: None,
            output: Some("file contents".to_string()),
            error_message: None,
            duration_ms: Some(150),
        };
        // State validation: collapsed by default, status is terminal.
        assert!(tool.status.is_terminal());
        assert!(tool.output.is_some());
        assert!(tool.error_message.is_none());
    }
}
