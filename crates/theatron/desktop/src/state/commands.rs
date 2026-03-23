//! Slash command state — client commands and server-provided agent commands.

/// Where a command originates.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CommandSource {
    /// Built into the desktop client.
    Client,
    /// Provided by the server for a specific agent.
    Server,
}

/// A slash command available in the command palette.
#[derive(Debug, Clone)]
pub struct Command {
    /// Command name without the leading `/`.
    pub name: String,
    /// Short description shown in the palette.
    pub description: String,
    /// Usage hint shown on selection, e.g. `/help [topic]`.
    pub usage: String,
    /// Where this command comes from.
    pub source: CommandSource,
    /// Whether this command is specific to the active agent.
    pub agent_specific: bool,
}

fn client_commands() -> Vec<Command> {
    vec![
        Command {
            name: "help".to_string(),
            description: "Show available commands and keyboard shortcuts".to_string(),
            usage: "/help".to_string(),
            source: CommandSource::Client,
            agent_specific: false,
        },
        Command {
            name: "clear".to_string(),
            description: "Clear the current chat history".to_string(),
            usage: "/clear".to_string(),
            source: CommandSource::Client,
            agent_specific: false,
        },
        Command {
            name: "theme".to_string(),
            description: "Toggle between light and dark themes".to_string(),
            usage: "/theme".to_string(),
            source: CommandSource::Client,
            agent_specific: false,
        },
        Command {
            name: "disconnect".to_string(),
            description: "Disconnect from the server".to_string(),
            usage: "/disconnect".to_string(),
            source: CommandSource::Client,
            agent_specific: false,
        },
    ]
}

/// Slash command palette state: full list and filtered view.
///
/// In Dioxus, this wraps into `Signal<CommandStore>` provided at the layout
/// level. Components read `filtered` and `cursor` to render the palette;
/// they call `filter_by_prefix`, `cursor_up`, `cursor_down` on write.
#[derive(Debug, Clone)]
pub struct CommandStore {
    /// All registered commands (client + server).
    all: Vec<Command>,
    /// Filtered subset currently shown in the palette.
    pub filtered: Vec<Command>,
    /// Highlighted row index into `filtered`.
    pub cursor: usize,
}

impl CommandStore {
    /// Create a store pre-loaded with client commands.
    #[must_use]
    pub(crate) fn new() -> Self {
        let all = client_commands();
        let filtered = all.clone();
        Self {
            all,
            filtered,
            cursor: 0,
        }
    }

    /// Merge server-provided commands, replacing any previous server commands.
    pub(crate) fn load_server_commands(&mut self, commands: Vec<Command>) {
        self.all.retain(|c| c.source != CommandSource::Server);
        self.all.extend(commands);
        self.filter_by_prefix("");
    }

    /// Filter commands by prefix (without leading `/`).
    ///
    /// Empty prefix shows all commands. Matching is case-insensitive on the
    /// command name.
    pub(crate) fn filter_by_prefix(&mut self, prefix: &str) {
        let lower = prefix.to_lowercase();
        self.filtered = self
            .all
            .iter()
            .filter(|c| lower.is_empty() || c.name.starts_with(lower.as_str()))
            .cloned()
            .collect();
        // Clamp cursor to valid range.
        if self.filtered.is_empty() {
            self.cursor = 0;
        } else if self.cursor >= self.filtered.len() {
            self.cursor = self.filtered.len() - 1;
        }
    }

    /// Move cursor up by one row (no-op at top).
    pub(crate) fn cursor_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    /// Move cursor down by one row (no-op at bottom).
    pub(crate) fn cursor_down(&mut self) {
        if !self.filtered.is_empty() && self.cursor < self.filtered.len() - 1 {
            self.cursor += 1;
        }
    }

    /// Currently selected command.
    #[must_use]
    pub(crate) fn selected(&self) -> Option<&Command> {
        self.filtered.get(self.cursor)
    }

    /// Whether the filtered list is empty.
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.filtered.is_empty()
    }
}

impl Default for CommandStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
#[expect(clippy::indexing_slicing, reason = "test assertions use direct indexing")]
mod tests {
    use super::*;

    fn server_cmd(name: &str) -> Command {
        Command {
            name: name.to_string(),
            description: format!("Server command: {name}"),
            usage: format!("/{name}"),
            source: CommandSource::Server,
            agent_specific: true,
        }
    }

    #[test]
    fn command_store_has_client_commands() {
        let store = CommandStore::new();
        assert!(!store.is_empty());
        assert!(store.filtered.iter().all(|c| c.source == CommandSource::Client));
    }

    #[test]
    fn filter_empty_prefix_shows_all() {
        let mut store = CommandStore::new();
        store.filter_by_prefix("");
        assert_eq!(store.filtered.len(), store.all.len());
    }

    #[test]
    fn filter_prefix_narrows_to_help() {
        let mut store = CommandStore::new();
        store.filter_by_prefix("hel");
        assert_eq!(store.filtered.len(), 1);
        assert_eq!(store.filtered[0].name, "help");
    }

    #[test]
    fn filter_no_match_empties_list() {
        let mut store = CommandStore::new();
        store.filter_by_prefix("zzz");
        assert!(store.is_empty());
        assert_eq!(store.cursor, 0);
    }

    #[test]
    fn filter_clamps_cursor_to_new_length() {
        let mut store = CommandStore::new();
        // Move to the last item then filter to a smaller set.
        let last = store.filtered.len() - 1;
        store.cursor = last;
        store.filter_by_prefix("hel"); // 1 result
        assert_eq!(store.cursor, 0);
    }

    #[test]
    fn cursor_navigation_down_and_up() {
        let mut store = CommandStore::new();
        store.filter_by_prefix(""); // All commands
        assert_eq!(store.cursor, 0);
        store.cursor_down();
        assert_eq!(store.cursor, 1);
        store.cursor_down();
        assert_eq!(store.cursor, 2);
        store.cursor_up();
        assert_eq!(store.cursor, 1);
        store.cursor_up();
        assert_eq!(store.cursor, 0);
    }

    #[test]
    fn cursor_up_at_top_is_noop() {
        let mut store = CommandStore::new();
        store.cursor_up();
        assert_eq!(store.cursor, 0);
    }

    #[test]
    fn cursor_down_at_bottom_is_noop() {
        let mut store = CommandStore::new();
        store.filter_by_prefix("help"); // 1 result, cursor 0 = bottom
        store.cursor_down();
        assert_eq!(store.cursor, 0);
    }

    #[test]
    fn selected_returns_highlighted_command() {
        let store = CommandStore::new();
        let sel = store.selected().unwrap();
        assert_eq!(sel.name, store.filtered[0].name);
    }

    #[test]
    fn selected_none_when_empty() {
        let mut store = CommandStore::new();
        store.filter_by_prefix("no-match-at-all");
        assert!(store.selected().is_none());
    }

    #[test]
    fn load_server_commands_appends() {
        let mut store = CommandStore::new();
        let initial = store.filtered.len();
        store.load_server_commands(vec![server_cmd("recall")]);
        assert_eq!(store.filtered.len(), initial + 1);
    }

    #[test]
    fn load_server_commands_replaces_prior_server_set() {
        let mut store = CommandStore::new();
        let client_count = store.all.len();
        store.load_server_commands(vec![server_cmd("cmd1"), server_cmd("cmd2")]);
        store.load_server_commands(vec![server_cmd("cmd3")]);
        // Only cmd3 should remain from server; client commands intact.
        let server_names: Vec<&str> = store
            .filtered
            .iter()
            .filter(|c| c.source == CommandSource::Server)
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(server_names, vec!["cmd3"]);
        assert_eq!(store.filtered.len(), client_count + 1);
    }
}
