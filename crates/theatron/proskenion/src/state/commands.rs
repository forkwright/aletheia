//! Slash command state -- client commands and server-provided agent commands.

use skene::id::NousId;

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
    /// Discovered from the server for an agent capability.
    Server,
}

/// Desktop route target for a navigation command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CommandDestination {
    /// Chat route.
    Chat,
    /// Theke file workspace route.
    Files,
    /// Planning route.
    Planning,
    /// Memory route.
    Memory,
    /// Metrics route.
    Metrics,
    /// Ops route.
    Ops,
    /// Sessions route.
    Sessions,
    /// Settings route.
    Settings,
}

/// Executable action associated with a command.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CommandAction {
    /// Open the keyboard help overlay.
    ShowHelp,
    /// Clear the current visible chat transcript.
    ClearChat,
    /// Toggle between dark and light themes.
    ToggleTheme,
    /// Disconnect from the configured server.
    Disconnect,
    /// Copy the current conversation to the clipboard as Markdown.
    ExportMarkdown,
    /// Navigate to a desktop route.
    Navigate(CommandDestination),
    /// Open details for a server-discovered agent tool.
    OpenToolDetails {
        /// Agent that advertised the tool capability.
        agent_id: NousId,
        /// Tool name as reported by the server.
        tool_name: String,
    },
}

/// A slash command available in the command palette.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// Disabled reason shown when a known command cannot run.
    pub disabled_reason: Option<String>,
    /// Execution action returned to the view layer.
    pub action: CommandAction,
}

/// Server-discovered command descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ServerCommandDescriptor {
    /// Agent that owns the advertised capability.
    pub agent_id: NousId,
    /// Human-readable agent label.
    pub agent_name: String,
    /// Tool name from the server discovery payload.
    pub tool_name: String,
    /// Tool description from the server discovery payload.
    pub description: String,
    /// Whether the server reports the capability as currently enabled.
    pub enabled: bool,
}

/// Slash command UI shell state shared by layout, input, and chat.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CommandUiState {
    /// Whether the command palette is visible.
    pub palette_open: bool,
    /// Whether the help overlay is visible.
    pub help_visible: bool,
}

/// A resolved command invocation ready for the view layer to apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommandInvocation {
    /// Command definition selected by slash input or the palette.
    pub command: Command,
    /// Original text submitted by the operator.
    pub raw: String,
    /// Arguments after the command name.
    pub args: String,
}

/// User-visible command execution state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CommandExecutionState {
    /// A known enabled command completed.
    Succeeded {
        /// Command name.
        name: String,
        /// User-facing result message.
        message: String,
    },
    /// A known enabled command failed while applying its action.
    Failed {
        /// Command name.
        name: String,
        /// User-facing failure message.
        message: String,
    },
    /// A known command was selected but is currently disabled.
    Disabled {
        /// Command name.
        name: String,
        /// User-facing disabled reason.
        reason: String,
    },
    /// No command matched the submitted name.
    Unknown {
        /// Submitted command name.
        name: String,
    },
}

/// Resolution result from slash input or palette selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CommandResolution {
    /// Command is known and enabled; apply the action.
    Ready(CommandInvocation),
    /// Command cannot run and the rejection was recorded in the store.
    Rejected(CommandExecutionState),
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
            disabled_reason: None,
            action: CommandAction::ShowHelp,
        },
        Command {
            name: "clear".to_string(),
            description: "Clear the current chat history".to_string(),
            usage: "/clear".to_string(),
            source: CommandSource::Client,
            agent_specific: false,
            category: CommandCategory::Action,
            disabled_reason: None,
            action: CommandAction::ClearChat,
        },
        Command {
            name: "theme".to_string(),
            description: "Toggle between light and dark themes".to_string(),
            usage: "/theme".to_string(),
            source: CommandSource::Client,
            agent_specific: false,
            category: CommandCategory::Action,
            disabled_reason: None,
            action: CommandAction::ToggleTheme,
        },
        Command {
            name: "disconnect".to_string(),
            description: "Disconnect from the server".to_string(),
            usage: "/disconnect".to_string(),
            source: CommandSource::Client,
            agent_specific: false,
            category: CommandCategory::Action,
            disabled_reason: None,
            action: CommandAction::Disconnect,
        },
        Command {
            name: "export".to_string(),
            description: "Export conversation to clipboard as markdown".to_string(),
            usage: "/export".to_string(),
            source: CommandSource::Client,
            agent_specific: false,
            category: CommandCategory::Action,
            disabled_reason: None,
            action: CommandAction::ExportMarkdown,
        },
        // --- Navigation commands ---
        Command {
            name: "chat".to_string(),
            description: "Switch to Chat view".to_string(),
            usage: "/chat".to_string(),
            source: CommandSource::Client,
            agent_specific: false,
            category: CommandCategory::Navigation,
            disabled_reason: None,
            action: CommandAction::Navigate(CommandDestination::Chat),
        },
        Command {
            name: "sessions".to_string(),
            description: "Switch to Sessions view".to_string(),
            usage: "/sessions".to_string(),
            source: CommandSource::Client,
            agent_specific: false,
            category: CommandCategory::Navigation,
            disabled_reason: None,
            action: CommandAction::Navigate(CommandDestination::Sessions),
        },
        Command {
            name: "memory".to_string(),
            description: "Switch to Memory view".to_string(),
            usage: "/memory".to_string(),
            source: CommandSource::Client,
            agent_specific: false,
            category: CommandCategory::Navigation,
            disabled_reason: None,
            action: CommandAction::Navigate(CommandDestination::Memory),
        },
        Command {
            name: "metrics".to_string(),
            description: "Switch to Metrics view".to_string(),
            usage: "/metrics".to_string(),
            source: CommandSource::Client,
            agent_specific: false,
            category: CommandCategory::Navigation,
            disabled_reason: None,
            action: CommandAction::Navigate(CommandDestination::Metrics),
        },
        Command {
            name: "ops".to_string(),
            description: "Switch to Ops view".to_string(),
            usage: "/ops".to_string(),
            source: CommandSource::Client,
            agent_specific: false,
            category: CommandCategory::Navigation,
            disabled_reason: None,
            action: CommandAction::Navigate(CommandDestination::Ops),
        },
        Command {
            name: "files".to_string(),
            description: "Switch to Theke (vault) view".to_string(),
            usage: "/files".to_string(),
            source: CommandSource::Client,
            agent_specific: false,
            category: CommandCategory::Navigation,
            disabled_reason: None,
            action: CommandAction::Navigate(CommandDestination::Files),
        },
        Command {
            name: "planning".to_string(),
            description: "Switch to Planning view".to_string(),
            usage: "/planning".to_string(),
            source: CommandSource::Client,
            agent_specific: false,
            category: CommandCategory::Navigation,
            disabled_reason: None,
            action: CommandAction::Navigate(CommandDestination::Planning),
        },
        Command {
            name: "settings".to_string(),
            description: "Switch to Settings view".to_string(),
            usage: "/settings".to_string(),
            source: CommandSource::Client,
            agent_specific: false,
            category: CommandCategory::Navigation,
            disabled_reason: None,
            action: CommandAction::Navigate(CommandDestination::Settings),
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
    /// Complete command registry.
    pub all: Vec<Command>,
    /// Filtered subset currently shown in the palette.
    pub filtered: Vec<Command>,
    /// Highlighted row index into `filtered`.
    pub cursor: usize,
    /// Last command execution state for UI feedback and tests.
    pub(crate) last_result: Option<CommandExecutionState>,
    filter: String,
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
            last_result: None,
            filter: String::new(),
        }
    }

    /// Replace all server-discovered commands with the latest backend payload.
    pub(crate) fn replace_server_commands(&mut self, descriptors: Vec<ServerCommandDescriptor>) {
        self.all
            .retain(|command| command.source != CommandSource::Server);
        self.all.extend(descriptors.into_iter().map(server_command));
        self.apply_filter();
    }

    /// Filter commands by slash/palette prefix and reset the cursor.
    pub(crate) fn filter_by_prefix(&mut self, prefix: &str) {
        self.filter = normalize_query(prefix);
        self.apply_filter();
    }

    /// Move selection up, clamped at the first row.
    pub(crate) fn cursor_up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    /// Move selection down, clamped at the last row.
    pub(crate) fn cursor_down(&mut self) {
        let max = self.filtered.len().saturating_sub(1);
        self.cursor = (self.cursor + 1).min(max);
    }

    /// Currently selected command.
    #[must_use]
    pub(crate) fn selected(&self) -> Option<&Command> {
        self.filtered.get(self.cursor)
    }

    /// Resolve a slash command string such as `/clear`.
    pub(crate) fn resolve_slash(&mut self, raw: &str) -> CommandResolution {
        let (name, args) = parse_command_input(raw);
        let Some(command) = self.command_by_name(&name).cloned() else {
            let state = CommandExecutionState::Unknown { name };
            self.last_result = Some(state.clone());
            return CommandResolution::Rejected(state);
        };
        self.resolve_command(command, raw.to_string(), args)
    }

    /// Resolve the command currently highlighted in the palette.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "WHY(#4869): palette resolution contract is covered by state tests; runtime submits selected slash text through the shared resolver"
        )
    )]
    pub(crate) fn resolve_selected(&mut self) -> CommandResolution {
        let Some(command) = self.selected().cloned() else {
            let state = CommandExecutionState::Unknown {
                name: String::new(),
            };
            self.last_result = Some(state.clone());
            return CommandResolution::Rejected(state);
        };
        let raw = command.usage.clone();
        self.resolve_command(command, raw, String::new())
    }

    /// Record a successful command side effect.
    pub(crate) fn record_success(&mut self, name: impl Into<String>, message: impl Into<String>) {
        self.last_result = Some(CommandExecutionState::Succeeded {
            name: name.into(),
            message: message.into(),
        });
    }

    /// Record a failed command side effect.
    pub(crate) fn record_failure(&mut self, name: impl Into<String>, message: impl Into<String>) {
        self.last_result = Some(CommandExecutionState::Failed {
            name: name.into(),
            message: message.into(),
        });
    }

    fn resolve_command(
        &mut self,
        command: Command,
        raw: String,
        args: String,
    ) -> CommandResolution {
        if let Some(reason) = command.disabled_reason.clone() {
            let state = CommandExecutionState::Disabled {
                name: command.name,
                reason,
            };
            self.last_result = Some(state.clone());
            return CommandResolution::Rejected(state);
        }

        CommandResolution::Ready(CommandInvocation { command, raw, args })
    }

    fn command_by_name(&self, name: &str) -> Option<&Command> {
        self.all
            .iter()
            .find(|command| command.name.eq_ignore_ascii_case(name))
    }

    fn apply_filter(&mut self) {
        self.filtered = if self.filter.is_empty() {
            self.all.clone()
        } else {
            self.all
                .iter()
                .filter(|command| command_matches(command, &self.filter))
                .cloned()
                .collect()
        };
        self.cursor = 0;
    }
}

impl Default for CommandStore {
    fn default() -> Self {
        Self::new()
    }
}

fn parse_command_input(raw: &str) -> (String, String) {
    let trimmed = raw.trim().trim_start_matches('/');
    let Some(idx) = trimmed.find(char::is_whitespace) else {
        return (trimmed.to_ascii_lowercase(), String::new());
    };
    let (name, args) = trimmed.split_at(idx);
    (name.to_ascii_lowercase(), args.trim().to_string())
}

fn normalize_query(prefix: &str) -> String {
    prefix
        .trim()
        .trim_start_matches('/')
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn command_matches(command: &Command, query: &str) -> bool {
    command.name.to_ascii_lowercase().starts_with(query)
        || command.description.to_ascii_lowercase().contains(query)
        || command.usage.to_ascii_lowercase().contains(query)
}

fn server_command(descriptor: ServerCommandDescriptor) -> Command {
    let command_name = format!(
        "tool:{}:{}",
        command_slug(&descriptor.agent_id),
        command_slug(&descriptor.tool_name)
    );
    let disabled_reason = if descriptor.enabled {
        None
    } else {
        Some(format!(
            "{} is disabled for {}",
            descriptor.tool_name, descriptor.agent_name
        ))
    };

    Command {
        name: command_name.clone(),
        description: format!("{} ({})", descriptor.description, descriptor.agent_name),
        usage: format!("/{command_name}"),
        source: CommandSource::Server,
        agent_specific: true,
        category: CommandCategory::Server,
        disabled_reason,
        action: CommandAction::OpenToolDetails {
            agent_id: descriptor.agent_id,
            tool_name: descriptor.tool_name,
        },
    }
}

fn command_slug(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions use direct indexing"
)]
mod tests {
    use super::*;

    #[test]
    fn selected_returns_highlighted_command() {
        let store = CommandStore::new();
        let sel = store.selected().unwrap();
        assert_eq!(sel.name, store.filtered[0].name);
    }

    #[test]
    fn filter_by_prefix_matches_command_names() {
        let mut store = CommandStore::new();
        store.filter_by_prefix("/mem");

        assert_eq!(store.filtered.len(), 1);
        assert_eq!(store.selected().unwrap().name, "memory");
    }

    #[test]
    fn cursor_movement_is_clamped_to_filtered_rows() {
        let mut store = CommandStore::new();
        store.filter_by_prefix("");

        store.cursor_down();
        assert_eq!(store.cursor, 1);
        store.cursor_up();
        assert_eq!(store.cursor, 0);
        store.cursor_up();
        assert_eq!(store.cursor, 0);

        for _ in 0..100 {
            store.cursor_down();
        }
        assert_eq!(store.cursor, store.filtered.len() - 1);
    }

    #[test]
    fn slash_and_palette_resolution_share_command_action() {
        let mut store = CommandStore::new();
        let slash = store.resolve_slash("/clear");

        store.filter_by_prefix("clear");
        let palette = store.resolve_selected();

        let CommandResolution::Ready(slash_invocation) = slash else {
            panic!("slash command should resolve");
        };
        let CommandResolution::Ready(palette_invocation) = palette else {
            panic!("palette command should resolve");
        };
        assert_eq!(
            slash_invocation.command.action,
            palette_invocation.command.action
        );
        assert_eq!(slash_invocation.command.action, CommandAction::ClearChat);
    }

    #[test]
    fn server_discovery_adds_capability_gated_commands() {
        let mut store = CommandStore::new();
        store.replace_server_commands(vec![ServerCommandDescriptor {
            agent_id: "syn".into(),
            agent_name: "Syn".to_string(),
            tool_name: "read_file".to_string(),
            description: "Read a workspace file".to_string(),
            enabled: false,
        }]);

        store.filter_by_prefix("tool:syn");
        let selected = store.selected().unwrap();
        assert_eq!(selected.source, CommandSource::Server);
        assert!(selected.agent_specific);

        let resolution = store.resolve_selected();
        let CommandResolution::Rejected(CommandExecutionState::Disabled { name, reason }) =
            resolution
        else {
            panic!("disabled server command should be rejected");
        };
        assert_eq!(name, "tool:syn:read_file");
        assert_eq!(reason, "read_file is disabled for Syn");
    }

    #[test]
    fn execution_states_distinguish_unknown_success_and_failure() {
        let mut store = CommandStore::new();

        let unknown = store.resolve_slash("/does-not-exist");
        assert_eq!(
            unknown,
            CommandResolution::Rejected(CommandExecutionState::Unknown {
                name: "does-not-exist".to_string(),
            })
        );

        store.record_success("clear", "Chat cleared");
        assert_eq!(
            store.last_result,
            Some(CommandExecutionState::Succeeded {
                name: "clear".to_string(),
                message: "Chat cleared".to_string(),
            })
        );

        store.record_failure("export", "Nothing to export");
        assert_eq!(
            store.last_result,
            Some(CommandExecutionState::Failed {
                name: "export".to_string(),
                message: "Nothing to export".to_string(),
            })
        );
    }
}
