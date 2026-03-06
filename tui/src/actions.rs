/// App action methods — message sending, tab completion, scroll state, cursor helpers.
use tracing::Instrument;

use crate::api::streaming;
use crate::app::App;
use crate::state::{ChatMessage, SavedScrollState, TabCompletion};

impl App {
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
                tokio::spawn(async move {
                    if let Err(e) = client.queue_message(&session_id, &text).await {
                        tracing::error!("failed to queue message: {e}");
                    }
                }.instrument(span));
            }
            return;
        }

        self.messages.push(ChatMessage {
            role: "user".to_string(),
            text: text.to_string(),
            timestamp: None,
            model: None,
            is_streaming: false,
            tool_calls: Vec::new(),
        });
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
