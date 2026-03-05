/// Live filter state — `/` mode for real-time content narrowing.

#[derive(Debug, Default, Clone, PartialEq)]
pub enum FilterScope {
    #[default]
    Chat,
    #[expect(dead_code, reason = "planned for sidebar filtering")]
    Agents,
}

#[derive(Debug, Default)]
pub struct FilterState {
    /// Whether filter mode is active (editing or applied)
    pub active: bool,
    /// Whether the user is currently typing in the filter bar
    pub editing: bool,
    /// Current filter text
    pub text: String,
    /// Cursor position in filter text (byte offset)
    pub cursor: usize,
    /// Which view the filter applies to
    pub scope: FilterScope,
    /// Number of matches in current view
    pub match_count: usize,
    /// Total items before filtering
    pub total_count: usize,
    /// Index of the currently highlighted match (for n/N navigation)
    pub current_match: usize,
}

impl FilterState {
    pub fn open(&mut self) {
        self.active = true;
        self.editing = true;
        self.text.clear();
        self.cursor = 0;
        self.match_count = 0;
        self.total_count = 0;
        self.current_match = 0;
        self.scope = FilterScope::Chat;
    }

    pub fn close(&mut self) {
        self.active = false;
        self.editing = false;
        self.text.clear();
        self.cursor = 0;
        self.match_count = 0;
        self.total_count = 0;
        self.current_match = 0;
    }

    pub fn confirm(&mut self) {
        self.editing = false;
    }

    pub fn insert_char(&mut self, c: char) {
        self.text.insert(self.cursor, c);
        self.cursor += c.len_utf8();
        self.current_match = 0;
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let prev = self.text[..self.cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.text.drain(prev..self.cursor);
            self.cursor = prev;
            self.current_match = 0;
        }
    }

    pub fn clear_text(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.match_count = 0;
        self.current_match = 0;
    }

    pub fn next_match(&mut self) {
        if self.match_count > 0 {
            self.current_match = (self.current_match + 1) % self.match_count;
        }
    }

    pub fn prev_match(&mut self) {
        if self.match_count > 0 {
            self.current_match = self
                .current_match
                .checked_sub(1)
                .unwrap_or(self.match_count - 1);
        }
    }

    /// Returns the effective pattern (without `!` prefix) and whether it's inverted.
    pub fn pattern(&self) -> (&str, bool) {
        if let Some(rest) = self.text.strip_prefix('!') {
            (rest, true)
        } else {
            (&self.text, false)
        }
    }
}
