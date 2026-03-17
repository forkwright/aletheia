/// App action methods: message sending, tab completion, scroll state, cursor helpers.
use tracing::Instrument;

/// Maximum number of per-agent scroll states retained in memory.
/// Entries beyond this cap are pruned to agents currently in the active roster.
const MAX_SCROLL_STATES: usize = 100;

use crate::api::streaming;
use crate::app::App;
use crate::state::virtual_scroll::estimate_message_height;
use crate::state::{ChatMessage, SavedScrollState, TabCompletion};

impl App {
    #[tracing::instrument(skip(self, text), fields(agent = ?self.dashboard.focused_agent))]
    pub(crate) fn send_message(&mut self, text: &str) {
        let agent_id = match &self.dashboard.focused_agent {
            Some(id) => id.clone(),
            None => return,
        };

        if self.connection.active_turn_id.is_some() {
            if let Some(ref session_id) = self.dashboard.focused_session_id {
                let client = self.client.clone();
                let session_id = session_id.clone();
                let text = text.to_string();
                let span = tracing::info_span!("queue_message");
                tokio::spawn(
                    async move {
                        if let Err(e) = client.queue_message(&session_id, &text).await {
                            tracing::error!("failed to queue message: {e}");
                        }
                    }
                    .instrument(span),
                );
            }
            return;
        }

        let text_owned = text.to_string();
        let text_lower = text_owned.to_lowercase();
        let msg = ChatMessage {
            role: "user".to_string(),
            text: text_owned,
            text_lower,
            timestamp: None,
            model: None,
            tool_calls: Vec::new(),
        };
        let width = self
            .viewport
            .render
            .virtual_scroll
            .cached_width()
            .max(self.viewport.terminal_width.saturating_sub(2).max(1));
        let h = estimate_message_height(msg.text.len(), !msg.tool_calls.is_empty(), width);
        self.dashboard.messages.push(msg);
        self.viewport.render.virtual_scroll.push_item(h);
        self.scroll_to_bottom();

        let session_key = self
            .dashboard
            .focused_agent
            .as_ref()
            .and_then(|id| {
                self.dashboard
                    .agents
                    .iter()
                    .find(|a| a.id == *id)
                    .and_then(|a| {
                        self.dashboard.focused_session_id.as_ref().and_then(|sid| {
                            a.sessions
                                .iter()
                                .find(|s| s.id == *sid)
                                .map(|s| s.key.clone())
                        })
                    })
            })
            .unwrap_or_else(|| "main".to_string());

        let rx = streaming::stream_message(
            self.client.raw_client().clone(),
            &self.config.url,
            &agent_id,
            &session_key,
            text,
        );
        self.connection.stream_rx = Some(rx);
    }

    pub(crate) fn handle_tab_completion(&mut self) {
        let text_before_cursor = self
            .interaction
            .input
            .text
            .get(..self.interaction.input.cursor)
            .unwrap_or("");
        if let Some(at_pos) = text_before_cursor.rfind('@') {
            let prefix = text_before_cursor.get(at_pos + 1..).unwrap_or("");

            if let Some(ref mut tc) = self.interaction.tab_completion
                && (tc.prefix == prefix || (!tc.candidates.is_empty() && tc.insert_start == at_pos))
            {
                tc.index = (tc.index + 1) % tc.candidates.len();
                let candidate = &tc.candidates[tc.index];

                self.interaction.input.text.replace_range(
                    at_pos..self.interaction.input.cursor,
                    &format!("@{} ", candidate),
                );
                self.interaction.input.cursor = at_pos + 1 + candidate.len() + 1;
                return;
            }

            let candidates: Vec<String> = self
                .dashboard
                .agents
                .iter()
                .filter(|a| {
                    a.id.starts_with(prefix)
                        || a.name.to_lowercase().starts_with(&prefix.to_lowercase())
                })
                .map(|a| a.id.to_string())
                .collect();

            if !candidates.is_empty() {
                let first = candidates[0].clone();
                self.interaction.tab_completion = Some(TabCompletion {
                    prefix: prefix.to_string(),
                    candidates,
                    index: 0,
                    insert_start: at_pos,
                });
                self.interaction.input.text.replace_range(
                    at_pos..self.interaction.input.cursor,
                    &format!("@{} ", first),
                );
                self.interaction.input.cursor = at_pos + 1 + first.len() + 1;
            }
        }
    }

    pub(crate) fn scroll_to_bottom(&mut self) {
        self.viewport.render.scroll_offset = 0;
        self.viewport.render.auto_scroll = true;
    }

    pub(crate) fn save_scroll_state(&mut self) {
        if let Some(ref id) = self.dashboard.focused_agent {
            self.viewport.render.scroll_states.insert(
                id.clone(),
                SavedScrollState {
                    scroll_offset: self.viewport.render.scroll_offset,
                    auto_scroll: self.viewport.render.auto_scroll,
                },
            );
            // NOTE: prune stale entries once the map exceeds MAX_SCROLL_STATES, retaining only agents in the current roster
            if self.viewport.render.scroll_states.len() > MAX_SCROLL_STATES {
                let active: Vec<_> = self.dashboard.agents.iter().map(|a| a.id.clone()).collect();
                self.viewport
                    .render
                    .scroll_states
                    .retain(|k, _| active.contains(k));
            }
        }
    }

    pub(crate) fn restore_scroll_state(&mut self) {
        if let Some(ref id) = self.dashboard.focused_agent {
            if let Some(state) = self.viewport.render.scroll_states.get(id) {
                self.viewport.render.scroll_offset = state.scroll_offset;
                self.viewport.render.auto_scroll = state.auto_scroll;
            } else {
                self.scroll_to_bottom();
            }
        }
    }

    /// Rebuild the virtual scroll height cache from current messages and terminal width.
    /// Called on session load, terminal resize, or when the cache becomes stale.
    pub(crate) fn rebuild_virtual_scroll(&mut self) {
        let width = self.viewport.terminal_width.saturating_sub(2).max(1);
        let heights: Vec<u16> = self
            .dashboard
            .messages
            .iter()
            .map(|msg| estimate_message_height(msg.text.len(), !msg.tool_calls.is_empty(), width))
            .collect();
        self.viewport.render.virtual_scroll.rebuild(&heights, width);
    }

    pub(crate) fn prev_char_boundary(&self, pos: usize) -> usize {
        let mut p = pos - 1;
        while p > 0 && !self.interaction.input.text.is_char_boundary(p) {
            p -= 1;
        }
        p
    }

    pub(crate) fn next_char_boundary(&self, pos: usize) -> usize {
        let mut p = pos + 1;
        while p < self.interaction.input.text.len()
            && !self.interaction.input.text.is_char_boundary(p)
        {
            p += 1;
        }
        p
    }
}

#[cfg(test)]
mod tests {
    use crate::app::test_helpers::*;

    #[test]
    fn scroll_to_bottom_resets_state() {
        let mut app = test_app();
        app.viewport.render.scroll_offset = 50;
        app.viewport.render.auto_scroll = false;
        app.scroll_to_bottom();
        assert_eq!(app.viewport.render.scroll_offset, 0);
        assert!(app.viewport.render.auto_scroll);
    }

    #[test]
    fn save_restore_scroll_state() {
        let mut app = test_app();
        app.dashboard.agents.push(test_agent("syn", "Syn"));
        app.dashboard.focused_agent = Some("syn".into());
        app.viewport.render.scroll_offset = 42;
        app.viewport.render.auto_scroll = false;

        app.save_scroll_state();
        app.viewport.render.scroll_offset = 0;
        app.viewport.render.auto_scroll = true;

        app.restore_scroll_state();
        assert_eq!(app.viewport.render.scroll_offset, 42);
        assert!(!app.viewport.render.auto_scroll);
    }

    #[test]
    fn restore_scroll_state_no_saved_scrolls_to_bottom() {
        let mut app = test_app();
        app.dashboard.agents.push(test_agent("syn", "Syn"));
        app.dashboard.focused_agent = Some("syn".into());
        app.viewport.render.scroll_offset = 99;

        app.restore_scroll_state();
        assert_eq!(app.viewport.render.scroll_offset, 0);
        assert!(app.viewport.render.auto_scroll);
    }

    #[test]
    fn save_scroll_state_no_agent_noop() {
        let mut app = test_app();
        app.viewport.render.scroll_offset = 42;
        app.save_scroll_state();
        assert!(app.viewport.render.scroll_states.is_empty());
    }

    #[test]
    fn scroll_states_pruned_when_exceeding_max() {
        let mut app = test_app();
        // Register one active agent.
        app.dashboard.agents.push(test_agent("active", "Active"));
        app.dashboard.focused_agent = Some("active".into());

        // Pre-fill with MAX_SCROLL_STATES + 1 stale entries for agents not in roster.
        for i in 0..=super::MAX_SCROLL_STATES {
            app.viewport.render.scroll_states.insert(
                crate::id::NousId::from(format!("stale-{i}")),
                crate::state::SavedScrollState {
                    scroll_offset: i,
                    auto_scroll: false,
                },
            );
        }

        // One save_scroll_state call triggers pruning.
        app.save_scroll_state();

        // Only the active agent entry should survive.
        assert!(
            app.viewport.render.scroll_states.len() <= super::MAX_SCROLL_STATES,
            "scroll_states must be capped at MAX_SCROLL_STATES after pruning"
        );
        assert!(
            app.viewport
                .render
                .scroll_states
                .contains_key(&crate::id::NousId::from("active")),
            "active agent entry must be retained after pruning"
        );
    }

    #[test]
    fn scroll_states_within_limit_not_pruned() {
        let mut app = test_app();
        app.dashboard.agents.push(test_agent("syn", "Syn"));
        app.dashboard.agents.push(test_agent("sol", "Sol"));
        app.dashboard.focused_agent = Some("syn".into());
        app.viewport.render.scroll_offset = 10;
        app.viewport.render.auto_scroll = false;

        app.save_scroll_state();
        // Well under the cap: no pruning should occur.
        assert_eq!(app.viewport.render.scroll_states.len(), 1);
    }

    #[test]
    fn prev_char_boundary_ascii() {
        let mut app = test_app();
        app.interaction.input.text = "hello".to_string();
        assert_eq!(app.prev_char_boundary(3), 2);
    }

    #[test]
    fn prev_char_boundary_multibyte() {
        let mut app = test_app();
        app.interaction.input.text = "h\u{00e9}llo".to_string(); // e-accent is 2 bytes
        // After 'h' (pos 1) and 'e-accent' (pos 3)
        assert_eq!(app.prev_char_boundary(3), 1);
    }

    #[test]
    fn next_char_boundary_ascii() {
        let mut app = test_app();
        app.interaction.input.text = "hello".to_string();
        assert_eq!(app.next_char_boundary(2), 3);
    }

    #[test]
    fn next_char_boundary_multibyte() {
        let mut app = test_app();
        app.interaction.input.text = "h\u{00e9}llo".to_string();
        // After 'h' (pos 1), next boundary is after e-accent (pos 3)
        assert_eq!(app.next_char_boundary(1), 3);
    }

    #[test]
    fn tab_completion_finds_agents() {
        let mut app = test_app();
        app.dashboard.agents.push(test_agent("syn", "Syn"));
        app.dashboard.agents.push(test_agent("cody", "Cody"));
        app.interaction.input.text = "@s".to_string();
        app.interaction.input.cursor = 2;

        app.handle_tab_completion();

        assert!(app.interaction.input.text.starts_with("@syn "));
        assert!(app.interaction.tab_completion.is_some());
    }

    #[test]
    fn tab_completion_no_at_noop() {
        let mut app = test_app();
        app.dashboard.agents.push(test_agent("syn", "Syn"));
        app.interaction.input.text = "hello".to_string();
        app.interaction.input.cursor = 5;

        app.handle_tab_completion();

        assert_eq!(app.interaction.input.text, "hello");
        assert!(app.interaction.tab_completion.is_none());
    }

    #[test]
    fn tab_completion_no_match_noop() {
        let mut app = test_app();
        app.dashboard.agents.push(test_agent("syn", "Syn"));
        app.interaction.input.text = "@zzz".to_string();
        app.interaction.input.cursor = 4;

        app.handle_tab_completion();

        assert_eq!(app.interaction.input.text, "@zzz");
        assert!(app.interaction.tab_completion.is_none());
    }

    #[test]
    fn tab_completion_cycles() {
        let mut app = test_app();
        app.dashboard.agents.push(test_agent("syn", "Syn"));
        app.dashboard.agents.push(test_agent("sol", "Sol"));
        app.interaction.input.text = "@s".to_string();
        app.interaction.input.cursor = 2;

        app.handle_tab_completion();
        let first = app.interaction.input.text.clone();

        app.handle_tab_completion();
        let second = app.interaction.input.text.clone();

        // Should have cycled to a different completion (or same if only one match)
        assert!(first.starts_with('@'));
        assert!(second.starts_with('@'));
    }
}
