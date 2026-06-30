//! OpsState struct definition and methods.

use std::collections::HashMap;

use crate::id::ToolId;

use super::OpsDiffEntry;
use super::helpers::{
    extract_primary_arg, parse_diff_from_output, truncate_error, unverified_tool_metadata,
};
use super::summary::OpsSummary;
use super::types::{
    FocusedPane, OpsAutoShow, OpsThinkingBlock, OpsToolCall, OpsToolStatus, ToolMetadata,
};

/// Full state for the operations pane.
#[derive(Debug, Clone)]
pub struct OpsState {
    /// Whether the pane is currently visible
    pub(crate) visible: bool,
    /// Width as percentage of terminal (0-100), default 40
    pub(crate) width_pct: u16,
    /// Which pane has keyboard focus
    pub(crate) focused_pane: FocusedPane,
    /// Auto-show behavior
    pub(crate) auto_show: OpsAutoShow,
    /// Scroll offset within the ops pane
    pub(crate) scroll_offset: usize,
    /// Currently selected item index (for j/k navigation)
    pub(crate) selected_item: Option<usize>,

    /// Accumulated thinking text during current turn
    pub(crate) thinking: OpsThinkingBlock,
    /// Tool calls during current turn
    pub(crate) tool_calls: Vec<OpsToolCall>,
    /// File diffs parsed from tool results
    pub(crate) diffs: Vec<OpsDiffEntry>,
    /// Aggregated KPI summary for the current turn.
    pub(crate) summary: OpsSummary,
    /// Wall-clock start time for the current turn (elapsed display).
    pub(crate) turn_started_at: Option<std::time::Instant>,
    /// When true, show all tool calls including successful ones. Default: false (show only errors).
    pub(crate) show_all_successful: bool,
    /// Server-owned tool metadata keyed by sanitized tool name.
    pub(crate) tool_metadata: HashMap<String, ToolMetadata>,
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
            show_all_successful: false,
            tool_metadata: HashMap::new(),
        }
    }
}

impl OpsState {
    /// Toggle visibility of the operations pane.
    pub(crate) fn toggle(&mut self) {
        self.visible = !self.visible;
        if !self.visible {
            self.focused_pane = FocusedPane::Chat;
        }
    }

    /// Show the pane (for auto-show on streaming start).
    pub(crate) fn auto_show_if_configured(&mut self) {
        if self.auto_show == OpsAutoShow::Auto {
            self.visible = true;
        }
    }

    /// Hide the pane (for auto-collapse on idle).
    pub(crate) fn auto_hide_if_configured(&mut self) {
        if self.auto_show == OpsAutoShow::Auto {
            self.visible = false;
            self.focused_pane = FocusedPane::Chat;
        }
    }

    /// Clear all turn-specific data.
    pub(crate) fn clear_turn(&mut self) {
        self.thinking.text.clear();
        self.thinking.collapsed = false;
        self.tool_calls.clear();
        self.diffs.clear();
        self.scroll_offset = 0;
        self.selected_item = None;
        self.summary = OpsSummary::default();
        self.turn_started_at = Some(std::time::Instant::now());
        self.show_all_successful = false;
    }

    /// Toggle visibility of all successful tool calls.
    pub(crate) fn toggle_show_all(&mut self) {
        self.show_all_successful = !self.show_all_successful;
    }

    /// Replace the server-owned metadata cache used for future tool calls.
    pub(crate) fn replace_tool_metadata(&mut self, metadata: HashMap<String, ToolMetadata>) {
        self.tool_metadata = metadata;
    }

    /// Switch keyboard focus between panes.
    pub(crate) fn toggle_focus(&mut self) {
        if self.visible {
            self.focused_pane = match self.focused_pane {
                FocusedPane::Chat => FocusedPane::Operations,
                FocusedPane::Operations => FocusedPane::Chat,
            };
        }
    }

    /// Total number of navigable items (thinking + tool calls).
    pub(crate) fn item_count(&self) -> usize {
        let thinking_items = usize::from(!self.thinking.text.is_empty());
        thinking_items + self.tool_calls.len()
    }

    /// Move selection up.
    pub(crate) fn select_prev(&mut self) {
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
    pub(crate) fn select_next(&mut self) {
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
    pub(crate) fn toggle_selected(&mut self) {
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
    pub(crate) fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_add(3);
    }

    /// Scroll the operations pane down.
    pub(crate) fn scroll_down(&mut self) {
        if self.scroll_offset >= 3 {
            self.scroll_offset -= 3;
        } else {
            self.scroll_offset = 0;
        }
    }

    /// Add a thinking delta.
    pub(crate) fn push_thinking(&mut self, text: &str) {
        self.thinking.text.push_str(text);
    }

    /// Start a new tool call.
    pub(crate) fn push_tool_start(&mut self, name: String, input_json: Option<String>) {
        self.push_tool_start_with_id(name, None, input_json);
    }

    /// Start a new stream tool call with its lifecycle id.
    pub(crate) fn push_tool_start_with_id(
        &mut self,
        name: String,
        tool_id: Option<ToolId>,
        input_json: Option<String>,
    ) {
        let primary_arg = input_json
            .as_deref()
            .and_then(|j| extract_primary_arg(j, &name));
        let metadata = self
            .tool_metadata
            .get(&name)
            .cloned()
            .unwrap_or_else(|| unverified_tool_metadata(&name));
        let category = metadata.category;
        self.tool_calls.push(OpsToolCall {
            name,
            tool_id,
            input_json,
            output: None,
            status: OpsToolStatus::Running,
            duration_ms: None,
            expanded: false,
            primary_arg,
            error_message: None,
            category,
            risk: metadata.risk,
            metadata,
            started_at: std::time::Instant::now(),
        });
    }

    /// Complete a tool call with result.
    pub(crate) fn complete_tool(
        &mut self,
        name: &str,
        is_error: bool,
        duration_ms: u64,
        output: Option<String>,
    ) {
        let Some(idx) = self.tool_calls.iter().rposition(|t| t.name == name) else {
            return;
        };
        self.complete_tool_at(idx, is_error, duration_ms, output);
    }

    /// Complete a stream tool call with result.
    pub(crate) fn complete_tool_by_id(
        &mut self,
        tool_id: &ToolId,
        is_error: bool,
        duration_ms: u64,
        output: Option<String>,
    ) {
        let Some(idx) = self
            .tool_calls
            .iter()
            .rposition(|t| t.tool_id.as_ref().is_some_and(|id| id == tool_id))
        else {
            return;
        };
        self.complete_tool_at(idx, is_error, duration_ms, output);
    }

    fn complete_tool_at(
        &mut self,
        idx: usize,
        is_error: bool,
        duration_ms: u64,
        output: Option<String>,
    ) {
        let name = self.tool_calls[idx].name.clone();
        let category = self.tool_calls[idx].category;
        let error_message = output
            .as_ref()
            .filter(|_| is_error)
            .map(|out| truncate_error(out));
        let diff = output
            .as_ref()
            .and_then(|out| parse_diff_from_output(out, &name));

        let tc = &mut self.tool_calls[idx];
        tc.status = if is_error {
            OpsToolStatus::Failed
        } else {
            OpsToolStatus::Complete
        };
        tc.duration_ms = Some(duration_ms);
        tc.error_message = error_message;
        tc.output = output;

        if let Some(diff) = diff {
            self.diffs.push(diff);
        }
        self.summary.record(category, is_error, duration_ms);
    }
}
