//! State for the operations pane: right-side panel showing thinking, tool calls, and diffs.

/// Which pane currently has keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FocusedPane {
    #[default]
    Chat,
    Operations,
}

/// Status of a tool call in the operations pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpsToolStatus {
    Running,
    Complete,
    Failed,
}

/// Tool category for grouping calls by type in the ops pane KPI display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolCategory {
    /// File reads: read_file, glob, grep, etc.
    Read,
    /// File writes: write_file, edit_file, notebook_edit, etc.
    Write,
    /// Search operations: web_search, search, etc.
    Search,
    /// Shell execution: bash, exec, etc.
    Exec,
    /// HTTP operations: web_fetch, http, etc.
    Http,
    /// Uncategorized tools.
    Other,
}

impl std::fmt::Display for ToolCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read => write!(f, "read"),
            Self::Write => write!(f, "write"),
            Self::Search => write!(f, "search"),
            Self::Exec => write!(f, "exec"),
            Self::Http => write!(f, "http"),
            Self::Other => write!(f, "other"),
        }
    }
}

/// Categorize a tool name into a [`ToolCategory`].
pub fn categorize_tool(name: &str) -> ToolCategory {
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

/// Per-category success/fail tallies and duration samples for percentile computation.
#[derive(Debug, Clone, Default)]
pub struct CategoryStats {
    pub success: u32,
    pub fail: u32,
    /// Sorted durations in milliseconds for percentile computation.
    durations: Vec<u64>,
}

impl CategoryStats {
    /// Record a completed tool call.
    pub fn record(&mut self, is_error: bool, duration_ms: u64) {
        if is_error {
            self.fail += 1;
        } else {
            self.success += 1;
        }
        // Insert in sorted order for percentile lookups.
        let pos = self.durations.partition_point(|&d| d < duration_ms);
        self.durations.insert(pos, duration_ms);
    }

    /// Total calls (success + fail).
    #[cfg(test)]
    pub fn total(&self) -> u32 {
        self.success + self.fail
    }

    /// Compute a percentile (0–100) from the sorted durations.
    /// Returns `None` if no durations have been recorded.
    pub fn percentile(&self, p: u8) -> Option<u64> {
        if self.durations.is_empty() {
            return None;
        }
        let idx = ((p as usize) * self.durations.len() / 100).min(self.durations.len() - 1);
        self.durations.get(idx).copied()
    }
}

/// Summary KPIs for the ops pane header row.
#[derive(Debug, Clone, Default)]
pub struct OpsSummary {
    pub total_calls: u32,
    pub total_errors: u32,
    /// Per-category statistics.
    pub categories: std::collections::HashMap<ToolCategory, CategoryStats>,
}

impl OpsSummary {
    /// Record a completed tool call into the summary.
    pub fn record(&mut self, category: ToolCategory, is_error: bool, duration_ms: u64) {
        self.total_calls += 1;
        if is_error {
            self.total_errors += 1;
        }
        self.categories
            .entry(category)
            .or_default()
            .record(is_error, duration_ms);
    }
}

/// A single tool call entry in the operations pane.
#[derive(Debug, Clone)]
pub struct OpsToolCall {
    pub name: String,
    pub input_json: Option<String>,
    pub output: Option<String>,
    pub status: OpsToolStatus,
    pub duration_ms: Option<u64>,
    pub expanded: bool,
    /// Primary argument extracted from input (path, command, pattern, etc.)
    pub primary_arg: Option<String>,
    /// Error summary for failed tool calls, extracted from result text.
    pub error_message: Option<String>,
    /// Tool category for KPI grouping.
    pub category: ToolCategory,
}

/// A single thinking block in the operations pane.
#[derive(Debug, Clone)]
pub struct OpsThinkingBlock {
    pub text: String,
    pub collapsed: bool,
}

/// A file diff entry parsed from tool results.
#[derive(Debug, Clone)]
pub struct OpsDiffEntry {
    pub file_path: String,
    pub additions: Vec<String>,
    pub deletions: Vec<String>,
}

/// Auto-show behavior configuration.
///
/// Additional variants (`Always`, `Manual`) will be added when config wiring lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OpsAutoShow {
    /// Show automatically when streaming starts, collapse when idle
    #[default]
    Auto,
}

/// Full state for the operations pane.
#[derive(Debug, Clone)]
pub struct OpsState {
    /// Whether the pane is currently visible
    pub visible: bool,
    /// Width as percentage of terminal (0-100), default 40
    pub width_pct: u16,
    /// Which pane has keyboard focus
    pub focused_pane: FocusedPane,
    /// Auto-show behavior
    pub auto_show: OpsAutoShow,
    /// Scroll offset within the ops pane
    pub scroll_offset: usize,
    /// Currently selected item index (for j/k navigation)
    pub selected_item: Option<usize>,

    /// Accumulated thinking text during current turn
    pub thinking: OpsThinkingBlock,
    /// Tool calls during current turn
    pub tool_calls: Vec<OpsToolCall>,
    /// File diffs parsed from tool results
    pub diffs: Vec<OpsDiffEntry>,
    /// Aggregated KPI summary for the current turn.
    pub summary: OpsSummary,
    /// Wall-clock start time for the current turn (elapsed display).
    pub turn_started_at: Option<std::time::Instant>,
}

impl Default for OpsState {
    fn default() -> Self {
        Self {
            visible: false,
            width_pct: 40,
            focused_pane: FocusedPane::default(),
            auto_show: OpsAutoShow::default(),
            scroll_offset: 0,
            selected_item: None,
            thinking: OpsThinkingBlock {
                text: String::new(),
                collapsed: false,
            },
            tool_calls: Vec::new(),
            diffs: Vec::new(),
            summary: OpsSummary::default(),
            turn_started_at: None,
        }
    }
}

impl OpsState {
    /// Toggle visibility of the operations pane.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
        if !self.visible {
            self.focused_pane = FocusedPane::Chat;
        }
    }

    /// Show the pane (for auto-show on streaming start).
    pub fn auto_show_if_configured(&mut self) {
        if self.auto_show == OpsAutoShow::Auto {
            self.visible = true;
        }
    }

    /// Hide the pane (for auto-collapse on idle).
    pub fn auto_hide_if_configured(&mut self) {
        if self.auto_show == OpsAutoShow::Auto {
            self.visible = false;
            self.focused_pane = FocusedPane::Chat;
        }
    }

    /// Clear all turn-specific data.
    pub fn clear_turn(&mut self) {
        self.thinking.text.clear();
        self.thinking.collapsed = false;
        self.tool_calls.clear();
        self.diffs.clear();
        self.scroll_offset = 0;
        self.selected_item = None;
        self.summary = OpsSummary::default();
        self.turn_started_at = Some(std::time::Instant::now());
    }

    /// Switch keyboard focus between panes.
    pub fn toggle_focus(&mut self) {
        if self.visible {
            self.focused_pane = match self.focused_pane {
                FocusedPane::Chat => FocusedPane::Operations,
                FocusedPane::Operations => FocusedPane::Chat,
            };
        }
    }

    /// Total number of navigable items (thinking + tool calls).
    pub fn item_count(&self) -> usize {
        let thinking_items = usize::from(!self.thinking.text.is_empty());
        thinking_items + self.tool_calls.len()
    }

    /// Move selection up.
    pub fn select_prev(&mut self) {
        let count = self.item_count();
        if count == 0 {
            return;
        }
        self.selected_item = Some(match self.selected_item {
            Some(i) => i.saturating_sub(1),
            None => count.saturating_sub(1),
        });
    }

    /// Move selection down.
    pub fn select_next(&mut self) {
        let count = self.item_count();
        if count == 0 {
            return;
        }
        self.selected_item = Some(match self.selected_item {
            Some(i) => (i + 1).min(count.saturating_sub(1)),
            None => 0,
        });
    }

    /// Toggle expansion of the selected item.
    pub fn toggle_selected(&mut self) {
        let Some(idx) = self.selected_item else {
            return;
        };
        let thinking_offset = if self.thinking.text.is_empty() {
            0
        } else if idx == 0 {
            self.thinking.collapsed = !self.thinking.collapsed;
            return;
        } else {
            1
        };
        let tool_idx = idx.saturating_sub(thinking_offset);
        if let Some(tc) = self.tool_calls.get_mut(tool_idx) {
            tc.expanded = !tc.expanded;
        }
    }

    /// Scroll the operations pane up.
    pub fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_add(3);
    }

    /// Scroll the operations pane down.
    pub fn scroll_down(&mut self) {
        if self.scroll_offset >= 3 {
            self.scroll_offset -= 3;
        } else {
            self.scroll_offset = 0;
        }
    }

    /// Add a thinking delta.
    pub fn push_thinking(&mut self, text: &str) {
        self.thinking.text.push_str(text);
    }

    /// Start a new tool call.
    pub fn push_tool_start(&mut self, name: String, input_json: Option<String>) {
        let primary_arg = input_json
            .as_deref()
            .and_then(|j| extract_primary_arg(j, &name));
        let category = categorize_tool(&name);
        self.tool_calls.push(OpsToolCall {
            name,
            input_json,
            output: None,
            status: OpsToolStatus::Running,
            duration_ms: None,
            expanded: false,
            primary_arg,
            error_message: None,
            category,
        });
    }

    /// Complete a tool call with result.
    pub fn complete_tool(
        &mut self,
        name: &str,
        is_error: bool,
        duration_ms: u64,
        output: Option<String>,
    ) {
        let category = if let Some(tc) = self.tool_calls.iter_mut().rev().find(|t| t.name == name) {
            tc.status = if is_error {
                OpsToolStatus::Failed
            } else {
                OpsToolStatus::Complete
            };
            tc.duration_ms = Some(duration_ms);
            if let Some(ref out) = output {
                if is_error {
                    tc.error_message = Some(truncate_error(out));
                }
                if let Some(diff) = parse_diff_from_output(out, name) {
                    self.diffs.push(diff);
                }
            }
            tc.output = output;
            tc.category
        } else {
            return;
        };
        self.summary.record(category, is_error, duration_ms);
    }
}

/// Maximum length for the inline primary arg display.
const PRIMARY_ARG_MAX_LEN: usize = 40;

/// Maximum length for the inline error summary.
const ERROR_MAX_LEN: usize = 80;

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

/// Extract the most informative argument from a tool's input JSON.
fn extract_primary_arg(json_str: &str, _tool_name: &str) -> Option<String> {
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

/// Truncate a string to `max_len` chars, appending "…" if truncated.
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len.saturating_sub(1)).collect();
        format!("{truncated}\u{2026}")
    }
}

/// Extract a one-line error summary from tool result text.
fn truncate_error(text: &str) -> String {
    let first_line = text.lines().next().unwrap_or(text);
    truncate_str(first_line, ERROR_MAX_LEN)
}

/// Try to parse a unified diff from a tool output string.
fn parse_diff_from_output(output: &str, tool_name: &str) -> Option<OpsDiffEntry> {
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

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
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
        assert!(arg.chars().count() <= PRIMARY_ARG_MAX_LEN);
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
        assert!(result.chars().count() <= ERROR_MAX_LEN);
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
