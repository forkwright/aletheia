/// Command registry and fuzzy matching for the `:` command palette.
use crate::fuzzy::fuzzy_match;
use crate::keybindings::{Action, KeyMap};
use crate::state::AgentState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CommandCategory {
    Navigation,
    Action,
    Query,
    Agent,
}

pub struct Command {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub description: &'static str,
    pub category: CommandCategory,
    /// The keymap action this command also performs, if any. The
    /// palette's shortcut badge is derived from this at suggestion-build
    /// time (`KeyMap::default_shortcut_display`) rather than hand-typed, so
    /// a command can never claim a chord it doesn't actually perform
    /// (#7222).
    pub action: Option<Action>,
}

#[derive(Debug)]
pub struct Suggestion {
    pub label: String,
    pub description: String,
    pub category: CommandCategory,
    pub aliases: &'static [&'static str],
    pub shortcut: Option<String>,
    pub score: i64,
    pub execute_as: String,
}

pub static COMMANDS: &[Command] = &[
    Command {
        name: "sessions",
        aliases: &["s"],
        description: "List sessions for current agent",
        category: CommandCategory::Navigation,
        action: Some(Action::OpenSessionPicker),
    },
    Command {
        name: "agents",
        aliases: &["a"],
        description: "Switch agent",
        category: CommandCategory::Navigation,
        action: Some(Action::OpenAgentPicker),
    },
    Command {
        name: "agent",
        aliases: &[],
        description: "Switch to named agent",
        category: CommandCategory::Agent,
        action: None,
    },
    Command {
        name: "cost",
        aliases: &["$"],
        description: "Show daily cost breakdown",
        category: CommandCategory::Query,
        action: Some(Action::OpenSystemStatus),
    },
    Command {
        name: "health",
        aliases: &["h"],
        description: "System health status",
        category: CommandCategory::Query,
        action: Some(Action::OpenSystemStatus),
    },
    // NOTE(#7203): `:compact` was deleted, not fixed -- there is no pylon route
    // that compacts/summarizes a session on demand (grepped `crates/pylon/src`
    // for `distill`/`compact`: only status/metrics fields exist, e.g.
    // `SessionStatus::Distilled`, `distillation_count` -- no
    // `POST /sessions/{id}/...` trigger in `crates/pylon/src/router.rs`).
    // A palette entry that can only ever toast "not available" is worse than
    // no entry.
    Command {
        name: "clear",
        aliases: &[],
        // WHY(#7222): this only wipes local view state (messages, streaming
        // buffers, the locally-tracked focused-session id) -- it never calls
        // the API, so it must not claim `:new`'s behavior or its Ctrl+N badge.
        description: "Clear local view only (server session is untouched)",
        category: CommandCategory::Action,
        action: None,
    },
    Command {
        name: "help",
        aliases: &["?"],
        description: "Show help",
        category: CommandCategory::Navigation,
        action: Some(Action::OpenHelp),
    },
    Command {
        name: "quit",
        aliases: &["q"],
        description: "Quit application",
        category: CommandCategory::Action,
        action: Some(Action::Quit),
    },
    Command {
        name: "recall",
        aliases: &["r"],
        description: "Search memory graph",
        category: CommandCategory::Query,
        action: None,
    },
    Command {
        name: "memory",
        aliases: &["mem", "m"],
        description: "Open memory inspector",
        category: CommandCategory::Navigation,
        action: Some(Action::MemoryOpen),
    },
    Command {
        name: "model",
        aliases: &[],
        description: "Show current model info",
        category: CommandCategory::Query,
        action: None,
    },
    Command {
        name: "settings",
        aliases: &[],
        description: "Open settings",
        category: CommandCategory::Navigation,
        action: None,
    },
    Command {
        name: "new",
        aliases: &[],
        description: "New conversation",
        category: CommandCategory::Action,
        action: Some(Action::NewSession),
    },
    Command {
        name: "rename",
        aliases: &[],
        description: "Rename current session",
        category: CommandCategory::Action,
        action: None,
    },
    Command {
        name: "archive",
        aliases: &[],
        description: "Archive current session",
        category: CommandCategory::Action,
        action: None,
    },
    Command {
        name: "unarchive",
        aliases: &[],
        description: "Restore archived session",
        category: CommandCategory::Action,
        action: None,
    },
    Command {
        name: "diff",
        aliases: &["d"],
        description: "Show uncommitted changes",
        category: CommandCategory::Query,
        action: None,
    },
    Command {
        name: "ops",
        aliases: &[],
        description: "Toggle operations pane",
        category: CommandCategory::Navigation,
        action: Some(Action::ToggleOpsPane),
    },
    Command {
        name: "tab",
        aliases: &[],
        description: "Switch to tab by name",
        category: CommandCategory::Navigation,
        action: None,
    },
    Command {
        name: "export",
        aliases: &[],
        description: "Export conversation to markdown (`export json` for a replay-faithful audit export)",
        category: CommandCategory::Action,
        action: None,
    },
    Command {
        name: "search",
        aliases: &[],
        description: "Search sessions and messages",
        category: CommandCategory::Query,
        action: None,
    },
    Command {
        name: "notifications",
        aliases: &["notif"],
        description: "View notification history",
        category: CommandCategory::Navigation,
        action: None,
    },
    Command {
        name: "metrics",
        aliases: &["stats"],
        description: "Open metrics dashboard",
        category: CommandCategory::Navigation,
        action: Some(Action::MetricsOpen),
    },
    Command {
        name: "editor",
        aliases: &["edit", "e"],
        description: "Open file editor",
        category: CommandCategory::Navigation,
        action: None,
    },
    Command {
        name: "reauth",
        aliases: &[],
        description: "Replace an expired/invalid token without restarting",
        category: CommandCategory::Action,
        action: None,
    },
    Command {
        name: "reconnect",
        aliases: &[],
        description: "Retry the gateway connection and reload agents",
        category: CommandCategory::Action,
        action: None,
    },
    Command {
        name: "context",
        aliases: &["budget"],
        description: "Show context window usage",
        category: CommandCategory::Query,
        action: None,
    },
];

const MAX_SUGGESTIONS: usize = 8;
// INVARIANT: derived from COMMANDS.len() so empty-input (initial open) always
// shows every static command -- a hand-picked constant silently falls behind
// the registry the moment a command is added (aletheia PR#7089: 25 hardcoded
// against 26 commands broke `empty_input_returns_all_commands`).
const MAX_SUGGESTIONS_INITIAL: usize = COMMANDS.len();

/// Build suggestions from static commands + dynamic agent entries.
pub fn build_suggestions(input: &str, agents: &[AgentState]) -> Vec<Suggestion> {
    let query = match input.split_once(' ') {
        Some((cmd, _)) => cmd,
        None => input,
    };

    let mut suggestions: Vec<Suggestion> = Vec::new();

    if query.is_empty() {
        for cmd in COMMANDS {
            suggestions.push(Suggestion {
                label: cmd.name.to_string(),
                description: cmd.description.to_string(),
                category: cmd.category,
                aliases: cmd.aliases,
                shortcut: cmd.action.and_then(KeyMap::default_shortcut_display),
                score: 0,
                execute_as: cmd.name.to_string(),
            });
        }
        for agent in agents {
            suggestions.push(agent_suggestion(agent, 0));
        }
    } else {
        for cmd in COMMANDS {
            if let Some(score) = best_match(cmd, query) {
                suggestions.push(Suggestion {
                    label: cmd.name.to_string(),
                    description: cmd.description.to_string(),
                    category: cmd.category,
                    aliases: cmd.aliases,
                    shortcut: cmd.action.and_then(KeyMap::default_shortcut_display),
                    score,
                    execute_as: cmd.name.to_string(),
                });
            }
        }

        for agent in agents {
            let mut best: Option<i64> = None;
            if let Some(result) = fuzzy_match(&agent.name, query) {
                best = Some(result.score);
            }
            if let Some(result) = fuzzy_match(&agent.id, query) {
                best = best.map_or(Some(result.score), |prev| Some(prev.max(result.score)));
            }
            // NOTE: Also match "agent <name>" as a compound so typing "agent syn" surfaces the agent.
            let compound = format!("agent {}", agent.name);
            if let Some(result) = fuzzy_match(&compound, query) {
                best = best.map_or(Some(result.score), |prev| Some(prev.max(result.score)));
            }
            if let Some(score) = best {
                suggestions.push(agent_suggestion(agent, score));
            }
        }
    }

    suggestions.sort_by_key(|s| std::cmp::Reverse(s.score));
    // NOTE: Show more results on empty input (initial open) than for active queries.
    let limit = if query.is_empty() {
        MAX_SUGGESTIONS_INITIAL
    } else {
        MAX_SUGGESTIONS
    };
    suggestions.truncate(limit);
    suggestions
}

fn agent_suggestion(agent: &AgentState, score: i64) -> Suggestion {
    let desc = match &agent.emoji {
        Some(emoji) => format!("{emoji} Switch to {}", agent.name),
        None => format!("Switch to {}", agent.name),
    };
    Suggestion {
        label: format!("agent {}", agent.id),
        description: desc,
        category: CommandCategory::Agent,
        aliases: &[],
        shortcut: None,
        score,
        execute_as: format!("agent {}", agent.id),
    }
}

fn best_match(cmd: &Command, query: &str) -> Option<i64> {
    let mut best: Option<i64> = None;

    if let Some(result) = fuzzy_match(cmd.name, query) {
        best = Some(result.score);
    }
    for alias in cmd.aliases {
        if let Some(result) = fuzzy_match(alias, query) {
            best = best.map_or(Some(result.score), |prev| Some(prev.max(result.score)));
        }
    }
    if let Some(result) = fuzzy_match(cmd.description, query) {
        best = best.map_or(Some(result.score), |prev| Some(prev.max(result.score)));
    }

    best
}

#[cfg(test)]
fn filter_commands(input: &str) -> Vec<Suggestion> {
    build_suggestions(input, &[])
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions use direct indexing for clarity"
)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_returns_all_commands() {
        let results = filter_commands("");
        assert!(results.len() >= COMMANDS.len());
    }

    #[test]
    fn exact_name_match_ranks_first() {
        let results = filter_commands("quit");
        assert!(!results.is_empty());
        assert_eq!(results[0].label, "quit");
    }

    #[test]
    fn alias_match_works() {
        let results = filter_commands("q");
        assert!(!results.is_empty());
        assert!(results.iter().any(|r| r.label == "quit"));
    }

    #[test]
    fn fuzzy_match_partial() {
        let results = filter_commands("sess");
        assert!(!results.is_empty());
        assert_eq!(results[0].label, "sessions");
    }

    #[test]
    fn max_eight_results() {
        let results = filter_commands("a");
        assert!(results.len() <= MAX_SUGGESTIONS);
    }

    #[test]
    fn command_with_args_matches_command_only() {
        let results = filter_commands("agent syn");
        assert!(!results.is_empty());
        assert!(results.iter().any(|r| r.label == "agent"));
    }

    #[test]
    fn dynamic_agents_appear_in_suggestions() {
        let agents = vec![AgentState {
            id: "syn".into(),
            name: "Syn".into(),
            name_lower: "syn".into(),
            emoji: Some("🧠".into()),
            status: crate::state::AgentStatus::Idle,
            backend_health: crate::state::BackendHealth::Healthy,
            active_tool: None,
            sessions: Vec::new(),
            model: Some("claude-opus-4-6".into()),
            compaction_stage: None,
            distill_completed_at: None,
            unread_count: 0,
            tools: Vec::new(),
            awaiting_approval_tool_id: None,
        }];
        let results = build_suggestions("syn", &agents);
        assert!(results.iter().any(|r| r.execute_as == "agent syn"));
    }

    #[test]
    fn shortcut_present_on_help() {
        let results = filter_commands("help");
        let help = results.iter().find(|r| r.label == "help").unwrap();
        assert_eq!(help.shortcut.as_deref(), Some("F1"));
    }

    #[test]
    fn sessions_command_has_ctrl_s_shortcut() {
        let results = filter_commands("sessions");
        let cmd = results.iter().find(|r| r.label == "sessions").unwrap();
        assert_eq!(cmd.shortcut.as_deref(), Some("Ctrl+S"));
    }

    /// Regression for #7222: `:clear`'s badge and description used to claim
    /// `:new`'s "Clear conversation / new session" behavior and `Ctrl+N`
    /// shortcut, even though its handler only wipes local view state and
    /// never calls the API. Deriving the badge from `Command::action` makes
    /// this class of bug structural: `:clear` has no `action`, so it can
    /// never carry a badge at all.
    #[test]
    fn clear_command_has_no_shortcut_and_does_not_claim_new_session() {
        let results = filter_commands("clear");
        let clear = results.iter().find(|r| r.label == "clear").unwrap();
        assert_eq!(clear.shortcut, None);
        assert!(
            !clear.description.to_lowercase().contains("new session"),
            "':clear' must not claim ':new'/Ctrl+N's behavior in its own description: {}",
            clear.description
        );
    }

    /// Regression for #7222 (generalized): every palette entry whose
    /// `action` derives a shortcut badge must have that exact chord actually
    /// registered somewhere in the keybinding registry (the same table the
    /// Help overlay and status bar read from) -- a badge is never allowed to
    /// drift ahead of, or diverge from, the real dispatch table.
    #[test]
    fn every_palette_shortcut_badge_matches_the_keybinding_registry() {
        use crate::keybindings::all_keybindings;

        let registry_keys: Vec<String> = all_keybindings()
            .iter()
            .map(|kb| kb.keys.to_lowercase().replace(' ', ""))
            .collect();

        for cmd in COMMANDS {
            let Some(action) = cmd.action else { continue };
            let badge = KeyMap::default_shortcut_display(action).unwrap_or_else(|| {
                panic!(
                    "command '{}' declares action {action:?} but the default keymap \
                     binds nothing to it -- the palette would show no badge at all",
                    cmd.name
                )
            });
            let normalized = badge.to_lowercase().replace(' ', "");
            assert!(
                registry_keys
                    .iter()
                    .any(|k| k.contains(normalized.as_str())),
                "command '{}' would show badge '[{badge}]' but no registry entry \
                 (Help overlay / status bar) contains that chord -- the badge and \
                 the documented keybinding have drifted apart",
                cmd.name
            );
        }
    }

    #[test]
    fn new_command_exists() {
        let results = filter_commands("new");
        assert!(results.iter().any(|r| r.label == "new"));
    }

    #[test]
    fn rename_command_exists() {
        let results = filter_commands("rename");
        assert!(results.iter().any(|r| r.label == "rename"));
    }

    #[test]
    fn archive_command_exists() {
        let results = filter_commands("archive");
        assert!(results.iter().any(|r| r.label == "archive"));
    }

    #[test]
    fn unarchive_command_exists() {
        let results = filter_commands("unarchive");
        assert!(results.iter().any(|r| r.label == "unarchive"));
    }
}
