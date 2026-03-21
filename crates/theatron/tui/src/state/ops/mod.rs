//! State for the operations pane: right-side panel showing thinking, tool calls, and diffs.

mod helpers;
mod state_impl;
mod summary;
mod types;

pub(crate) use helpers::categorize_tool;
pub use state_impl::OpsState;
pub(crate) use summary::{CategoryStats, OpsSummary};
pub use types::{FocusedPane, OpsAutoShow, OpsToolStatus};
pub(crate) use types::{OpsDiffEntry, OpsThinkingBlock, OpsToolCall, ToolCategory};

#[cfg(test)]
pub(crate) use helpers::{extract_primary_arg, parse_diff_from_output, truncate_error};

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions use direct indexing for clarity"
)]
mod tests {
    use super::*;

    #[test]
    fn default_state_is_hidden() {
        let state = OpsState::default();
        assert!(!state.visible);
        assert_eq!(state.width_pct, 40);
        assert_eq!(state.focused_pane, FocusedPane::Chat);
    }

    #[test]
    fn toggle_visibility() {
        let mut state = OpsState::default();
        state.toggle();
        assert!(state.visible);
        state.toggle();
        assert!(!state.visible);
    }

    #[test]
    fn toggle_resets_focus_on_hide() {
        let mut state = OpsState {
            visible: true,
            focused_pane: FocusedPane::Operations,
            ..OpsState::default()
        };
        state.toggle();
        assert!(!state.visible);
        assert_eq!(state.focused_pane, FocusedPane::Chat);
    }

    #[test]
    fn auto_show_auto_mode() {
        let mut state = OpsState::default();
        state.auto_show_if_configured();
        assert!(state.visible);
    }

    #[test]
    fn auto_hide_auto_mode() {
        let mut state = OpsState {
            visible: true,
            focused_pane: FocusedPane::Operations,
            ..OpsState::default()
        };
        state.auto_hide_if_configured();
        assert!(!state.visible);
        assert_eq!(state.focused_pane, FocusedPane::Chat);
    }

    #[test]
    fn clear_turn_resets_data() {
        let mut state = OpsState::default();
        state.thinking.text = "some thinking".to_string();
        state.tool_calls.push(OpsToolCall {
            name: "test".to_string(),
            input_json: None,
            output: None,
            status: OpsToolStatus::Running,
            duration_ms: None,
            expanded: false,
            primary_arg: None,
            error_message: None,
            category: ToolCategory::Other,
        });
        state.scroll_offset = 10;
        state.selected_item = Some(0);

        state.clear_turn();

        assert!(state.thinking.text.is_empty());
        assert!(state.tool_calls.is_empty());
        assert_eq!(state.scroll_offset, 0);
        assert!(state.selected_item.is_none());
        assert_eq!(state.summary.total_calls, 0);
        assert!(state.turn_started_at.is_some());
    }

    #[test]
    fn toggle_focus_between_panes() {
        let mut state = OpsState {
            visible: true,
            ..OpsState::default()
        };
        assert_eq!(state.focused_pane, FocusedPane::Chat);
        state.toggle_focus();
        assert_eq!(state.focused_pane, FocusedPane::Operations);
        state.toggle_focus();
        assert_eq!(state.focused_pane, FocusedPane::Chat);
    }

    #[test]
    fn toggle_focus_noop_when_hidden() {
        let mut state = OpsState::default();
        state.toggle_focus();
        assert_eq!(state.focused_pane, FocusedPane::Chat);
    }

    #[test]
    fn item_count_empty() {
        let state = OpsState::default();
        assert_eq!(state.item_count(), 0);
    }

    #[test]
    fn item_count_thinking_only() {
        let mut state = OpsState::default();
        state.thinking.text = "thinking...".to_string();
        assert_eq!(state.item_count(), 1);
    }

    #[test]
    fn item_count_thinking_plus_tools() {
        let mut state = OpsState::default();
        state.thinking.text = "thinking...".to_string();
        state.push_tool_start("read_file".to_string(), None);
        state.push_tool_start("write_file".to_string(), None);
        assert_eq!(state.item_count(), 3);
    }

    #[test]
    fn select_prev_from_none_selects_last() {
        let mut state = OpsState::default();
        state.push_tool_start("a".to_string(), None);
        state.push_tool_start("b".to_string(), None);
        state.select_prev();
        assert_eq!(state.selected_item, Some(1));
    }

    #[test]
    fn select_prev_saturates_at_zero() {
        let mut state = OpsState::default();
        state.push_tool_start("a".to_string(), None);
        state.selected_item = Some(0);
        state.select_prev();
        assert_eq!(state.selected_item, Some(0));
    }

    #[test]
    fn select_next_from_none_selects_first() {
        let mut state = OpsState::default();
        state.push_tool_start("a".to_string(), None);
        state.select_next();
        assert_eq!(state.selected_item, Some(0));
    }

    #[test]
    fn select_next_clamps_at_max() {
        let mut state = OpsState::default();
        state.push_tool_start("a".to_string(), None);
        state.push_tool_start("b".to_string(), None);
        state.selected_item = Some(1);
        state.select_next();
        assert_eq!(state.selected_item, Some(1));
    }

    #[test]
    fn toggle_selected_thinking() {
        let mut state = OpsState::default();
        state.thinking.text = "some thinking".to_string();
        state.selected_item = Some(0);
        assert!(!state.thinking.collapsed);
        state.toggle_selected();
        assert!(state.thinking.collapsed);
    }

    #[test]
    fn toggle_selected_tool_call() {
        let mut state = OpsState::default();
        state.push_tool_start("read_file".to_string(), None);
        state.selected_item = Some(0);
        assert!(!state.tool_calls[0].expanded);
        state.toggle_selected();
        assert!(state.tool_calls[0].expanded);
    }

    #[test]
    fn scroll_up_increases_offset() {
        let mut state = OpsState::default();
        state.scroll_up();
        assert_eq!(state.scroll_offset, 3);
    }

    #[test]
    fn scroll_down_decreases_offset() {
        let mut state = OpsState {
            scroll_offset: 10,
            ..OpsState::default()
        };
        state.scroll_down();
        assert_eq!(state.scroll_offset, 7);
    }

    #[test]
    fn scroll_down_floors_at_zero() {
        let mut state = OpsState {
            scroll_offset: 2,
            ..OpsState::default()
        };
        state.scroll_down();
        assert_eq!(state.scroll_offset, 0);
    }

    #[test]
    fn push_thinking_appends() {
        let mut state = OpsState::default();
        state.push_thinking("hello ");
        state.push_thinking("world");
        assert_eq!(state.thinking.text, "hello world");
    }

    #[test]
    fn push_tool_start_creates_running_entry() {
        let mut state = OpsState::default();
        state.push_tool_start("grep".to_string(), Some(r#"{"pattern":"foo"}"#.to_string()));
        assert_eq!(state.tool_calls.len(), 1);
        assert_eq!(state.tool_calls[0].name, "grep");
        assert_eq!(state.tool_calls[0].status, OpsToolStatus::Running);
        assert!(state.tool_calls[0].input_json.is_some());
    }

    #[test]
    fn complete_tool_success() {
        let mut state = OpsState::default();
        state.push_tool_start("read_file".to_string(), None);
        state.complete_tool("read_file", false, 150, Some("file contents".to_string()));
        assert_eq!(state.tool_calls[0].status, OpsToolStatus::Complete);
        assert_eq!(state.tool_calls[0].duration_ms, Some(150));
        assert_eq!(state.tool_calls[0].output.as_deref(), Some("file contents"));
    }

    #[test]
    fn complete_tool_failure() {
        let mut state = OpsState::default();
        state.push_tool_start("write_file".to_string(), None);
        state.complete_tool("write_file", true, 50, None);
        assert_eq!(state.tool_calls[0].status, OpsToolStatus::Failed);
    }

    #[test]
    fn parse_diff_from_edit_tool() {
        let output = "--- a/src/main.rs\n+++ b/src/main.rs\n-old line\n+new line\n";
        let diff = parse_diff_from_output(output, "edit_file");
        assert!(diff.is_some());
        let diff = diff.unwrap();
        assert_eq!(diff.file_path, "a/src/main.rs");
        assert_eq!(diff.additions.len(), 1);
        assert_eq!(diff.deletions.len(), 1);
    }

    #[test]
    fn parse_diff_ignores_non_file_tools() {
        let output = "--- a/src/main.rs\n+new line\n";
        let diff = parse_diff_from_output(output, "grep");
        assert!(diff.is_none());
    }

    #[test]
    fn parse_diff_no_changes() {
        let output = "no diff content here";
        let diff = parse_diff_from_output(output, "edit_file");
        assert!(diff.is_none());
    }

    #[test]
    fn complete_tool_extracts_diff() {
        let mut state = OpsState::default();
        state.push_tool_start("edit_file".to_string(), None);
        let output = "--- a/lib.rs\n+++ b/lib.rs\n-old\n+new\n";
        state.complete_tool("edit_file", false, 100, Some(output.to_string()));
        assert_eq!(state.diffs.len(), 1);
        assert_eq!(state.diffs[0].file_path, "a/lib.rs");
    }

    #[test]
    fn select_noop_on_empty() {
        let mut state = OpsState::default();
        state.select_next();
        assert!(state.selected_item.is_none());
        state.select_prev();
        assert!(state.selected_item.is_none());
    }

    #[test]
    fn toggle_selected_noop_no_selection() {
        let mut state = OpsState::default();
        state.push_tool_start("a".to_string(), None);
        state.toggle_selected(); // selected_item is None
        assert!(!state.tool_calls[0].expanded);
    }

    #[test]
    fn width_pct_default() {
        let state = OpsState::default();
        assert_eq!(state.width_pct, 40);
    }

    #[test]
    fn ops_auto_show_default_is_auto() {
        assert_eq!(OpsAutoShow::default(), OpsAutoShow::Auto);
    }

    #[test]
    fn extract_primary_arg_path() {
        let json = r#"{"file_path":"/src/main.rs","content":"fn main() {}"}"#;
        let arg = extract_primary_arg(json, "read_file");
        assert_eq!(arg.as_deref(), Some("/src/main.rs"));
    }

    #[test]
    fn extract_primary_arg_command() {
        let json = r#"{"command":"cargo test","timeout":30000}"#;
        let arg = extract_primary_arg(json, "exec");
        assert_eq!(arg.as_deref(), Some("cargo test"));
    }

    #[test]
    fn extract_primary_arg_pattern() {
        let json = r#"{"pattern":"fn main","path":"src/"}"#;
        // "path" comes before "pattern" in priority order
        let arg = extract_primary_arg(json, "grep");
        assert_eq!(arg.as_deref(), Some("src/"));
    }

    #[test]
    fn extract_primary_arg_none_for_empty_json() {
        let json = r#"{}"#;
        let arg = extract_primary_arg(json, "some_tool");
        assert!(arg.is_none());
    }

    #[test]
    fn extract_primary_arg_truncates_long_values() {
        let mut long_path = "/".to_string();
        long_path.push_str(&"a".repeat(100));
        let json = format!(r#"{{"file_path":"{long_path}"}}"#);
        let arg = extract_primary_arg(&json, "read_file").unwrap();
        assert!(arg.chars().count() <= 40);
        assert!(arg.ends_with('\u{2026}'));
    }

    #[test]
    fn push_tool_start_extracts_primary_arg() {
        let mut state = OpsState::default();
        let input = r#"{"file_path":"src/lib.rs"}"#.to_string();
        state.push_tool_start("read_file".to_string(), Some(input));
        assert_eq!(
            state.tool_calls[0].primary_arg.as_deref(),
            Some("src/lib.rs")
        );
    }

    #[test]
    fn complete_tool_error_extracts_message() {
        let mut state = OpsState::default();
        state.push_tool_start("exec".to_string(), None);
        state.complete_tool(
            "exec",
            true,
            200,
            Some("Permission denied: /etc/shadow\ndetailed trace...".to_string()),
        );
        assert_eq!(state.tool_calls[0].status, OpsToolStatus::Failed);
        assert_eq!(
            state.tool_calls[0].error_message.as_deref(),
            Some("Permission denied: /etc/shadow")
        );
    }

    #[test]
    fn complete_tool_success_no_error_message() {
        let mut state = OpsState::default();
        state.push_tool_start("read_file".to_string(), None);
        state.complete_tool("read_file", false, 150, Some("file contents".to_string()));
        assert!(state.tool_calls[0].error_message.is_none());
    }

    #[test]
    fn truncate_error_takes_first_line() {
        let text = "line one\nline two\nline three";
        assert_eq!(truncate_error(text), "line one");
    }

    #[test]
    fn truncate_error_truncates_long_line() {
        let long = "x".repeat(200);
        let result = truncate_error(&long);
        assert!(result.chars().count() <= helpers::ERROR_MAX_LEN);
        assert!(result.ends_with('\u{2026}'));
    }

    #[test]
    fn categorize_tool_read() {
        assert_eq!(categorize_tool("read_file"), ToolCategory::Read);
        assert_eq!(categorize_tool("Glob"), ToolCategory::Read);
        assert_eq!(categorize_tool("Grep"), ToolCategory::Read);
    }

    #[test]
    fn categorize_tool_write() {
        assert_eq!(categorize_tool("write_file"), ToolCategory::Write);
        assert_eq!(categorize_tool("Edit"), ToolCategory::Write);
        assert_eq!(categorize_tool("NotebookEdit"), ToolCategory::Write);
    }

    #[test]
    fn categorize_tool_exec() {
        assert_eq!(categorize_tool("Bash"), ToolCategory::Exec);
        assert_eq!(categorize_tool("exec_command"), ToolCategory::Exec);
    }

    #[test]
    fn categorize_tool_search() {
        assert_eq!(categorize_tool("web_search"), ToolCategory::Search);
    }

    #[test]
    fn categorize_tool_http() {
        assert_eq!(categorize_tool("web_fetch"), ToolCategory::Http);
    }

    #[test]
    fn categorize_tool_other() {
        assert_eq!(categorize_tool("agent"), ToolCategory::Other);
    }

    #[test]
    fn category_stats_percentile() {
        let mut stats = CategoryStats::default();
        stats.record(false, 10);
        stats.record(false, 20);
        stats.record(false, 30);
        stats.record(false, 40);
        stats.record(true, 50);
        assert_eq!(stats.success, 4);
        assert_eq!(stats.fail, 1);
        assert_eq!(stats.total(), 5);
        // p50 = index 2 (of 5 elements) = 30
        assert_eq!(stats.percentile(50), Some(30));
        // p95 = index 4 = 50
        assert_eq!(stats.percentile(95), Some(50));
    }

    #[test]
    fn category_stats_empty_percentile() {
        let stats = CategoryStats::default();
        assert_eq!(stats.percentile(50), None);
    }

    #[test]
    fn summary_records_across_categories() {
        let mut summary = OpsSummary::default();
        summary.record(ToolCategory::Read, false, 100);
        summary.record(ToolCategory::Read, false, 200);
        summary.record(ToolCategory::Write, true, 50);
        assert_eq!(summary.total_calls, 3);
        assert_eq!(summary.total_errors, 1);
        assert_eq!(summary.categories[&ToolCategory::Read].success, 2);
        assert_eq!(summary.categories[&ToolCategory::Write].fail, 1);
    }

    #[test]
    fn complete_tool_updates_summary() {
        let mut state = OpsState::default();
        state.push_tool_start("read_file".to_string(), None);
        state.complete_tool("read_file", false, 150, None);
        assert_eq!(state.summary.total_calls, 1);
        assert_eq!(state.summary.total_errors, 0);
        assert!(state.summary.categories.contains_key(&ToolCategory::Read));
    }

    #[test]
    fn push_tool_start_assigns_category() {
        let mut state = OpsState::default();
        state.push_tool_start("Bash".to_string(), None);
        assert_eq!(state.tool_calls[0].category, ToolCategory::Exec);
    }
}
