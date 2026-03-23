//! Input state for the chat input bar.

use std::collections::VecDeque;

/// Maximum number of messages retained in the input history ring buffer.
const MAX_HISTORY: usize = 50;

/// Tracks the current submission lifecycle.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SubmissionState {
    /// Ready to accept input.
    Idle,
    /// A message is being sent and streamed.
    Submitting,
    /// The last submission failed.
    Error(String),
}

impl Default for SubmissionState {
    fn default() -> Self {
        Self::Idle
    }
}

/// Input state for the chat textarea.
///
/// Manages the current text, submission lifecycle, and a ring buffer of
/// previously sent messages for up/down arrow history navigation.
#[derive(Debug, Clone)]
pub struct InputState {
    /// Current text content of the textarea.
    pub text: String,
    /// Ring buffer of previously submitted messages, newest at back.
    history: VecDeque<String>,
    /// Index into history during navigation. `None` means the user is
    /// editing fresh input (not browsing history).
    history_index: Option<usize>,
    /// Stashed draft text saved when the user starts navigating history,
    /// restored when they return past the newest entry.
    draft: String,
    /// Current submission lifecycle state.
    pub submission: SubmissionState,
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            text: String::new(),
            history: VecDeque::with_capacity(MAX_HISTORY),
            history_index: None,
            draft: String::new(),
            submission: SubmissionState::default(),
        }
    }
}

impl InputState {
    /// Push a submitted message into the history ring buffer.
    ///
    /// Drops the oldest entry when the buffer exceeds [`MAX_HISTORY`].
    /// Resets the history navigation index.
    pub(crate) fn push_history(&mut self, message: String) {
        if message.is_empty() {
            return;
        }
        // Deduplicate consecutive identical messages.
        if self.history.back().is_some_and(|last| last == &message) {
            self.history_index = None;
            return;
        }
        if self.history.len() >= MAX_HISTORY {
            self.history.pop_front();
        }
        self.history.push_back(message);
        self.history_index = None;
    }

    /// Navigate to the previous (older) history entry.
    ///
    /// On the first press, stashes the current draft text. Returns `true`
    /// if the text was changed.
    #[must_use]
    pub(crate) fn history_prev(&mut self) -> bool {
        if self.history.is_empty() {
            return false;
        }

        let new_index = match self.history_index {
            None => {
                self.draft = self.text.clone();
                self.history.len().saturating_sub(1)
            }
            Some(0) => return false,
            Some(idx) => idx.saturating_sub(1),
        };

        self.history_index = Some(new_index);
        if let Some(entry) = self.history.get(new_index) {
            self.text = entry.clone();
            true
        } else {
            false
        }
    }

    /// Navigate to the next (newer) history entry.
    ///
    /// When moving past the newest entry, restores the stashed draft.
    /// Returns `true` if the text was changed.
    #[must_use]
    pub(crate) fn history_next(&mut self) -> bool {
        let Some(idx) = self.history_index else {
            return false;
        };

        if idx >= self.history.len().saturating_sub(1) {
            // Past the newest entry: restore draft.
            self.history_index = None;
            self.text = std::mem::take(&mut self.draft);
            return true;
        }

        let new_index = idx + 1;
        self.history_index = Some(new_index);
        if let Some(entry) = self.history.get(new_index) {
            self.text = entry.clone();
            true
        } else {
            false
        }
    }

    /// Number of entries in the history buffer.
    #[must_use]
    pub(crate) fn history_len(&self) -> usize {
        self.history.len()
    }

    /// Whether the user is currently browsing history.
    #[must_use]
    pub(crate) fn is_browsing_history(&self) -> bool {
        self.history_index.is_some()
    }

    /// Clear the input text and reset history navigation.
    pub(crate) fn clear(&mut self) {
        self.text.clear();
        self.history_index = None;
        self.draft.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_and_navigate_history() {
        let mut input = InputState::default();
        input.push_history("first".into());
        input.push_history("second".into());
        input.push_history("third".into());

        assert_eq!(input.history_len(), 3);

        // Navigate backwards through history.
        input.text = "draft".into();
        assert!(input.history_prev());
        assert_eq!(input.text, "third");
        assert!(input.history_prev());
        assert_eq!(input.text, "second");
        assert!(input.history_prev());
        assert_eq!(input.text, "first");

        // Cannot go further back.
        assert!(!input.history_prev());
        assert_eq!(input.text, "first");

        // Navigate forward restores draft.
        assert!(input.history_next());
        assert_eq!(input.text, "second");
        assert!(input.history_next());
        assert_eq!(input.text, "third");
        assert!(input.history_next());
        assert_eq!(input.text, "draft");

        // Cannot go further forward.
        assert!(!input.history_next());
    }

    #[test]
    fn history_ring_buffer_evicts_oldest() {
        let mut input = InputState::default();
        for i in 0..60 {
            input.push_history(format!("msg-{i}"));
        }
        assert_eq!(input.history_len(), MAX_HISTORY);
        // Oldest messages (0-9) should have been evicted.
        assert!(input.history_prev());
        assert_eq!(input.text, "msg-59");
    }

    #[test]
    fn empty_message_not_pushed() {
        let mut input = InputState::default();
        input.push_history(String::new());
        assert_eq!(input.history_len(), 0);
    }

    #[test]
    fn consecutive_duplicates_deduplicated() {
        let mut input = InputState::default();
        input.push_history("same".into());
        input.push_history("same".into());
        assert_eq!(input.history_len(), 1);
    }

    #[test]
    fn history_nav_on_empty_is_noop() {
        let mut input = InputState::default();
        assert!(!input.history_prev());
        assert!(!input.history_next());
    }

    #[test]
    fn clear_resets_state() {
        let mut input = InputState::default();
        input.text = "hello".into();
        input.push_history("old".into());
        let _ = input.history_prev();
        input.clear();
        assert!(input.text.is_empty());
        assert!(!input.is_browsing_history());
    }

    #[test]
    fn submission_state_default_is_idle() {
        assert_eq!(SubmissionState::default(), SubmissionState::Idle);
    }
}
