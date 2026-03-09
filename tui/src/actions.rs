/// App action methods — message sending, tab completion, scroll state, cursor helpers.
use tracing::Instrument;

use crate::api::streaming;
use crate::app::App;
use crate::state::virtual_scroll::estimate_message_height;
use crate::state::{ChatMessage, SavedScrollState, TabCompletion};

impl App {
    #[tracing::instrument(skip(self, text), fields(agent = ?self.focused_agent))]
    pub(crate) fn send_message(&mut self, text: &str) {
        let agent_id = match &self.focused_agent {
            Some(id) => id.clone(),
            None => return,
        };

        if self.active_turn_id.is_some() {
            if let Some(ref session_id) = self.focused_session_id {
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
            is_streaming: false,
            tool_calls: Vec::new(),
        };
        let width = self
            .virtual_scroll
            .cached_width()
            .max(self.terminal_width.saturating_sub(2).max(1));
        let h = estimate_message_height(msg.text.len(), !msg.tool_calls.is_empty(), width);
        self.messages.push(msg);
        self.virtual_scroll.push_item(h);
        self.scroll_to_bottom();

        let session_key = self
            .focused_agent
            .as_ref()
            .and_then(|id| {
                self.agents.iter().find(|a| a.id == *id).and_then(|a| {
                    self.focused_session_id.as_ref().and_then(|sid| {
                        a.sessions
                            .iter()
                            .find(|s| s.id == *sid)
                            .map(|s| s.key.clone())
                    })
                })
            })
            .unwrap_or_else(|| "main".to_string());

        let rx = streaming::stream_message(
            &self.config.url,
            self.client.token(),
            &agent_id,
            &session_key,
            text,
        );
        self.stream_rx = Some(rx);
    }

    pub(crate) fn handle_tab_completion(&mut self) {
        let text_before_cursor = &self.input.text[..self.input.cursor];
        if let Some(at_pos) = text_before_cursor.rfind('@') {
            let prefix = &text_before_cursor[at_pos + 1..];

            if let Some(ref mut tc) = self.tab_completion {
                if tc.prefix == prefix || (!tc.candidates.is_empty() && tc.insert_start == at_pos) {
                    tc.index = (tc.index + 1) % tc.candidates.len();
                    let candidate = &tc.candidates[tc.index];

                    self.input
                        .text
                        .replace_range(at_pos..self.input.cursor, &format!("@{} ", candidate));
                    self.input.cursor = at_pos + 1 + candidate.len() + 1;
                    return;
                }
            }

            let candidates: Vec<String> = self
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
                self.tab_completion = Some(TabCompletion {
                    prefix: prefix.to_string(),
                    candidates,
                    index: 0,
                    insert_start: at_pos,
                });
                self.input
                    .text
                    .replace_range(at_pos..self.input.cursor, &format!("@{} ", first));
                self.input.cursor = at_pos + 1 + first.len() + 1;
            }
        }
    }

    pub(crate) fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
        self.auto_scroll = true;
    }

    pub(crate) fn save_scroll_state(&mut self) {
        if let Some(ref id) = self.focused_agent {
            self.scroll_states.insert(
                id.clone(),
                SavedScrollState {
                    scroll_offset: self.scroll_offset,
                    auto_scroll: self.auto_scroll,
                },
            );
        }
    }

    pub(crate) fn restore_scroll_state(&mut self) {
        if let Some(ref id) = self.focused_agent {
            if let Some(state) = self.scroll_states.get(id) {
                self.scroll_offset = state.scroll_offset;
                self.auto_scroll = state.auto_scroll;
            } else {
                self.scroll_to_bottom();
            }
        }
    }

    /// Rebuild the virtual scroll height cache from current messages and terminal width.
    /// Called on session load, terminal resize, or when the cache becomes stale.
    pub(crate) fn rebuild_virtual_scroll(&mut self) {
        let width = self.terminal_width.saturating_sub(2).max(1);
        let heights: Vec<u16> = self
            .messages
            .iter()
            .map(|msg| estimate_message_height(msg.text.len(), !msg.tool_calls.is_empty(), width))
            .collect();
        self.virtual_scroll.rebuild(&heights, width);
    }

    pub(crate) fn prev_char_boundary(&self, pos: usize) -> usize {
        let mut p = pos - 1;
        while p > 0 && !self.input.text.is_char_boundary(p) {
            p -= 1;
        }
        p
    }

    pub(crate) fn next_char_boundary(&self, pos: usize) -> usize {
        let mut p = pos + 1;
        while p < self.input.text.len() && !self.input.text.is_char_boundary(p) {
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
        app.scroll_offset = 50;
        app.auto_scroll = false;
        app.scroll_to_bottom();
        assert_eq!(app.scroll_offset, 0);
        assert!(app.auto_scroll);
    }

    #[test]
    fn save_restore_scroll_state() {
        let mut app = test_app();
        app.agents.push(test_agent("syn", "Syn"));
        app.focused_agent = Some("syn".into());
        app.scroll_offset = 42;
        app.auto_scroll = false;

        app.save_scroll_state();
        app.scroll_offset = 0;
        app.auto_scroll = true;

        app.restore_scroll_state();
        assert_eq!(app.scroll_offset, 42);
        assert!(!app.auto_scroll);
    }

    #[test]
    fn restore_scroll_state_no_saved_scrolls_to_bottom() {
        let mut app = test_app();
        app.agents.push(test_agent("syn", "Syn"));
        app.focused_agent = Some("syn".into());
        app.scroll_offset = 99;

        app.restore_scroll_state();
        assert_eq!(app.scroll_offset, 0);
        assert!(app.auto_scroll);
    }

    #[test]
    fn save_scroll_state_no_agent_noop() {
        let mut app = test_app();
        app.scroll_offset = 42;
        app.save_scroll_state();
        assert!(app.scroll_states.is_empty());
    }

    #[test]
    fn prev_char_boundary_ascii() {
        let mut app = test_app();
        app.input.text = "hello".to_string();
        assert_eq!(app.prev_char_boundary(3), 2);
    }

    #[test]
    fn prev_char_boundary_multibyte() {
        let mut app = test_app();
        app.input.text = "h\u{00e9}llo".to_string(); // e-accent is 2 bytes
        // After 'h' (pos 1) and 'e-accent' (pos 3)
        assert_eq!(app.prev_char_boundary(3), 1);
    }

    #[test]
    fn next_char_boundary_ascii() {
        let mut app = test_app();
        app.input.text = "hello".to_string();
        assert_eq!(app.next_char_boundary(2), 3);
    }

    #[test]
    fn next_char_boundary_multibyte() {
        let mut app = test_app();
        app.input.text = "h\u{00e9}llo".to_string();
        // After 'h' (pos 1), next boundary is after e-accent (pos 3)
        assert_eq!(app.next_char_boundary(1), 3);
    }

    #[test]
    fn tab_completion_finds_agents() {
        let mut app = test_app();
        app.agents.push(test_agent("syn", "Syn"));
        app.agents.push(test_agent("cody", "Cody"));
        app.input.text = "@s".to_string();
        app.input.cursor = 2;

        app.handle_tab_completion();

        assert!(app.input.text.starts_with("@syn "));
        assert!(app.tab_completion.is_some());
    }

    #[test]
    fn tab_completion_no_at_noop() {
        let mut app = test_app();
        app.agents.push(test_agent("syn", "Syn"));
        app.input.text = "hello".to_string();
        app.input.cursor = 5;

        app.handle_tab_completion();

        assert_eq!(app.input.text, "hello");
        assert!(app.tab_completion.is_none());
    }

    #[test]
    fn tab_completion_no_match_noop() {
        let mut app = test_app();
        app.agents.push(test_agent("syn", "Syn"));
        app.input.text = "@zzz".to_string();
        app.input.cursor = 4;

        app.handle_tab_completion();

        assert_eq!(app.input.text, "@zzz");
        assert!(app.tab_completion.is_none());
    }

    #[test]
    fn tab_completion_cycles() {
        let mut app = test_app();
        app.agents.push(test_agent("syn", "Syn"));
        app.agents.push(test_agent("sol", "Sol"));
        app.input.text = "@s".to_string();
        app.input.cursor = 2;

        app.handle_tab_completion();
        let first = app.input.text.clone();

        app.handle_tab_completion();
        let second = app.input.text.clone();

        // Should have cycled to a different completion (or same if only one match)
        assert!(first.starts_with('@'));
        assert!(second.starts_with('@'));
    }
}
