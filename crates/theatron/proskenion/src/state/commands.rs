//! Slash command state -- client commands and server-provided agent commands.

/// Where a command originates.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CommandSource {
    /// Built into the desktop client.
    Client,
    /// Provided by the server for a specific agent.
    Server,
}

/// Functional category for palette grouping and dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CommandCategory {
    /// Performs an action (export, clear, etc.).
    Action,
    /// Navigates to a different view.
    Navigation,
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
    /// Functional category for dispatch.
    pub category: CommandCategory,
}

fn client_commands() -> Vec<Command> {
    vec![
        // --- Action commands ---
        Command {
            name: "help".to_string(),
            description: "Show available commands and keyboard shortcuts".to_string(),
            usage: "/help".to_string(),
            source: CommandSource::Client,
            agent_specific: false,
            category: CommandCategory::Action,
        },
        Command {
            name: "clear".to_string(),
            description: "Clear the current chat history".to_string(),
            usage: "/clear".to_string(),
            source: CommandSource::Client,
            agent_specific: false,
            category: CommandCategory::Action,
        },
        Command {
            name: "theme".to_string(),
            description: "Toggle between light and dark themes".to_string(),
            usage: "/theme".to_string(),
            source: CommandSource::Client,
            agent_specific: false,
            category: CommandCategory::Action,
        },
        Command {
            name: "disconnect".to_string(),
            description: "Disconnect from the server".to_string(),
            usage: "/disconnect".to_string(),
            source: CommandSource::Client,
            agent_specific: false,
            category: CommandCategory::Action,
        },
        Command {
            name: "export".to_string(),
            description: "Export conversation to clipboard as markdown".to_string(),
            usage: "/export".to_string(),
            source: CommandSource::Client,
            agent_specific: false,
            category: CommandCategory::Action,
        },
        // --- Navigation commands ---
        Command {
            name: "sessions".to_string(),
            description: "Switch to Sessions view".to_string(),
            usage: "/sessions".to_string(),
            source: CommandSource::Client,
            agent_specific: false,
            category: CommandCategory::Navigation,
        },
        Command {
            name: "memory".to_string(),
            description: "Switch to Memory view".to_string(),
            usage: "/memory".to_string(),
            source: CommandSource::Client,
            agent_specific: false,
            category: CommandCategory::Navigation,
        },
        Command {
            name: "metrics".to_string(),
            description: "Switch to Metrics view".to_string(),
            usage: "/metrics".to_string(),
            source: CommandSource::Client,
            agent_specific: false,
            category: CommandCategory::Navigation,
        },
        Command {
            name: "ops".to_string(),
            description: "Switch to Ops view".to_string(),
            usage: "/ops".to_string(),
            source: CommandSource::Client,
            agent_specific: false,
            category: CommandCategory::Navigation,
        },
        Command {
            name: "files".to_string(),
            description: "Switch to Files view".to_string(),
            usage: "/files".to_string(),
            source: CommandSource::Client,
            agent_specific: false,
            category: CommandCategory::Navigation,
        },
        Command {
            name: "planning".to_string(),
            description: "Switch to Planning view".to_string(),
            usage: "/planning".to_string(),
            source: CommandSource::Client,
            agent_specific: false,
            category: CommandCategory::Navigation,
        },
        Command {
            name: "settings".to_string(),
            description: "Switch to Settings view".to_string(),
            usage: "/settings".to_string(),
            source: CommandSource::Client,
            agent_specific: false,
            category: CommandCategory::Navigation,
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
            filtered,
            cursor: 0,
        }
    }

    /// Currently selected command.
    #[must_use]
    pub(crate) fn selected(&self) -> Option<&Command> {
        self.filtered.get(self.cursor)
    }
}

impl Default for CommandStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions use direct indexing"
)]
mod tests {
    use super::*;

    fn server_cmd(name: &str) -> Command {
        Command {
            name: name.to_string(),
            description: format!("Server command: {name}"),
            usage: format!("/{name}"),
            source: CommandSource::Server,
            agent_specific: true,
            category: CommandCategory::Action,
        }
    }

    #[test]
    fn selected_returns_highlighted_command() {
        let store = CommandStore::new();
        let sel = store.selected().unwrap();
        assert_eq!(sel.name, store.filtered[0].name);
    }
}
