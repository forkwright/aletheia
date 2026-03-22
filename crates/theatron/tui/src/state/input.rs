const MAX_KILL_RING_ENTRIES: usize = 60;

#[derive(Debug, Default, Clone)]
pub struct InputState {
    pub text: String,
    pub cursor: usize,
    pub history: Vec<String>,
    pub history_index: Option<usize>,
    pub kill_ring: KillRing,
    pub history_search: Option<HistorySearchState>,
    pub image_attachments: Vec<ImageAttachment>,
}

#[derive(Debug)]
pub struct TabCompletion {
    pub prefix: String,
    pub candidates: Vec<String>,
    pub index: usize,
    pub insert_start: usize,
}

#[derive(Debug, Clone, Default)]
pub struct KillRing {
    pub(crate) entries: Vec<String>,
    /// Tracks the byte span of the last yank for Alt+Y replacement.
    pub(crate) last_yank: Option<YankSpan>,
}

impl KillRing {
    pub(crate) fn push(&mut self, text: String) {
        if text.is_empty() {
            return;
        }
        self.entries.push(text);
        if self.entries.len() > MAX_KILL_RING_ENTRIES {
            self.entries.remove(0);
        }
        self.last_yank = None;
    }

    pub(crate) fn last(&self) -> Option<&str> {
        self.entries.last().map(String::as_str)
    }

    /// Return the next (older) kill ring entry for Alt+Y cycling.
    pub(crate) fn cycle(&self, current_index: usize) -> Option<(usize, &str)> {
        if self.entries.is_empty() {
            return None;
        }
        let next = if current_index == 0 {
            self.entries.len() - 1
        } else {
            current_index - 1
        };
        self.entries.get(next).map(|s| (next, s.as_str()))
    }
}

#[derive(Debug, Clone)]
pub struct YankSpan {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) ring_index: usize,
}

#[derive(Debug, Clone)]
pub struct HistorySearchState {
    pub query: String,
    pub match_index: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct ImageAttachment {
    pub data: Vec<u8>,
    #[expect(
        dead_code,
        reason = "stored for API payload construction when sending image attachments"
    )]
    pub mime_type: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone)]
pub struct QueuedMessage {
    pub text: String,
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    #[test]
    fn kill_ring_push_and_last() {
        let mut kr = KillRing::default();
        assert!(kr.last().is_none());
        kr.push("hello".to_string());
        assert_eq!(kr.last(), Some("hello"));
        kr.push("world".to_string());
        assert_eq!(kr.last(), Some("world"));
    }

    #[test]
    fn kill_ring_push_empty_is_noop() {
        let mut kr = KillRing::default();
        kr.push(String::new());
        assert!(kr.entries.is_empty());
    }

    #[test]
    fn kill_ring_caps_at_max() {
        let mut kr = KillRing::default();
        for i in 0..=MAX_KILL_RING_ENTRIES {
            kr.push(format!("entry-{i}"));
        }
        assert_eq!(kr.entries.len(), MAX_KILL_RING_ENTRIES);
        assert_eq!(kr.entries.first().unwrap(), "entry-1");
    }

    #[test]
    fn kill_ring_cycle_wraps_around() {
        let mut kr = KillRing::default();
        kr.push("a".to_string());
        kr.push("b".to_string());
        kr.push("c".to_string());
        // Start at index 2 ("c"), cycle to 1 ("b")
        let (idx, text) = kr.cycle(2).unwrap();
        assert_eq!(idx, 1);
        assert_eq!(text, "b");
        // Cycle from 0 wraps to 2
        let (idx, text) = kr.cycle(0).unwrap();
        assert_eq!(idx, 2);
        assert_eq!(text, "c");
    }

    #[test]
    fn kill_ring_cycle_empty_returns_none() {
        let kr = KillRing::default();
        assert!(kr.cycle(0).is_none());
    }

    #[test]
    fn history_search_state_default() {
        let state = HistorySearchState {
            query: String::new(),
            match_index: None,
        };
        assert!(state.query.is_empty());
        assert!(state.match_index.is_none());
    }
}
