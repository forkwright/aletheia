/// Command registry and fuzzy matching for the `:` command palette.
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;

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
    pub shortcut: Option<&'static str>,
}

#[derive(Debug)]
pub struct Suggestion {
    pub label: String,
    pub description: String,
    pub category: CommandCategory,
    pub aliases: &'static [&'static str],
    pub shortcut: Option<&'static str>,
    pub score: i64,
    pub execute_as: String,
}

pub static COMMANDS: &[Command] = &[
    Command {
        name: "sessions",
        aliases: &["s"],
        description: "List sessions for current agent",
        category: CommandCategory::Navigation,
        shortcut: Some("Ctrl+S"),
    },
    Command {
        name: "agents",
        aliases: &["a"],
        description: "Switch agent",
        category: CommandCategory::Navigation,
        shortcut: Some("Ctrl+A"),
    },
    Command {
        name: "agent",
        aliases: &[],
        description: "Switch to named agent",
        category: CommandCategory::Agent,
        shortcut: None,
    },
    Command {
        name: "cost",
        aliases: &["$"],
        description: "Show daily cost breakdown",
        category: CommandCategory::Query,
        shortcut: Some("Ctrl+I"),
    },
    Command {
        name: "health",
        aliases: &["h"],
        description: "System health status",
        category: CommandCategory::Query,
        shortcut: Some("Ctrl+I"),
    },
    Command {
        name: "compact",
        aliases: &[],
        description: "Trigger distillation",
        category: CommandCategory::Action,
        shortcut: None,
    },
    Command {
        name: "clear",
        aliases: &[],
        description: "Clear conversation / new session",
        category: CommandCategory::Action,
        shortcut: Some("Ctrl+N"),
    },
    Command {
        name: "help",
        aliases: &["?"],
        description: "Show help",
        category: CommandCategory::Navigation,
        shortcut: Some("F1"),
    },
    Command {
        name: "quit",
        aliases: &["q"],
        description: "Quit application",
        category: CommandCategory::Action,
        shortcut: Some("Ctrl+C"),
    },
    Command {
        name: "recall",
        aliases: &["r"],
        description: "Search memory graph",
        category: CommandCategory::Query,
        shortcut: None,
    },
    Command {
        name: "memory",
        aliases: &["mem", "m"],
        description: "Open memory inspector",
        category: CommandCategory::Navigation,
        shortcut: Some("Ctrl+M"),
    },
    Command {
        name: "model",
        aliases: &[],
        description: "Show current model info",
        category: CommandCategory::Query,
        shortcut: None,
    },
    Command {
        name: "settings",
        aliases: &[],
        description: "Open settings",
        category: CommandCategory::Navigation,
        shortcut: None,
    },
    Command {
        name: "new",
        aliases: &[],
        description: "New conversation",
        category: CommandCategory::Action,
        shortcut: Some("Ctrl+N"),
    },
    Command {
        name: "rename",
        aliases: &[],
        description: "Rename current session",
        category: CommandCategory::Action,
        shortcut: None,
    },
    Command {
        name: "archive",
        aliases: &[],
        description: "Archive current session",
        category: CommandCategory::Action,
        shortcut: None,
    },
    Command {
        name: "unarchive",
        aliases: &[],
        description: "Restore archived session",
        category: CommandCategory::Action,
        shortcut: None,
    },
    Command {
        name: "diff",
        aliases: &["d"],
        description: "Show uncommitted changes",
        category: CommandCategory::Query,
        shortcut: None,
    },
    Command {
        name: "ops",
        aliases: &[],
        description: "Toggle operations pane",
        category: CommandCategory::Navigation,
        shortcut: Some("Ctrl+O"),
    },
    Command {
        name: "tab",
        aliases: &[],
        description: "Switch to tab by name",
        category: CommandCategory::Navigation,
        shortcut: None,
    },
    Command {
        name: "export",
        aliases: &[],
        description: "Export conversation to markdown",
        category: CommandCategory::Action,
        shortcut: None,
    },
    Command {
        name: "search",
        aliases: &[],
        description: "Search sessions and messages",
        category: CommandCategory::Query,
        shortcut: None,
    },
    Command {
        name: "notifications",
        aliases: &["notif"],
        description: "View notification history",
        category: CommandCategory::Navigation,
        shortcut: None,
    },
];

const MAX_SUGGESTIONS: usize = 8;
const MAX_SUGGESTIONS_INITIAL: usize = 25;

/// Build suggestions from static commands + dynamic agent entries.
pub fn build_suggestions(input: &str, agents: &[AgentState]) -> Vec<Suggestion> {
    let query = match input.split_once(' ') {
        Some((cmd, _)) => cmd,
        None => input,
    };

    let matcher = SkimMatcherV2::default();
    let mut suggestions: Vec<Suggestion> = Vec::new();

    if query.is_empty() {
        for cmd in COMMANDS {
            suggestions.push(Suggestion {
                label: cmd.name.to_string(),
                description: cmd.description.to_string(),
                category: cmd.category,
                aliases: cmd.aliases,
                shortcut: cmd.shortcut,
                score: 0,
                execute_as: cmd.name.to_string(),
            });
        }
        for agent in agents {
            suggestions.push(agent_suggestion(agent, 0));
        }
    } else {
        for cmd in COMMANDS {
            if let Some(score) = best_match(&matcher, cmd, query) {
                suggestions.push(Suggestion {
                    label: cmd.name.to_string(),
                    description: cmd.description.to_string(),
                    category: cmd.category,
                    aliases: cmd.aliases,
                    shortcut: cmd.shortcut,
                    score,
                    execute_as: cmd.name.to_string(),
                });
            }
        }

        for agent in agents {
            let mut best: Option<i64> = None;
            if let Some(s) = matcher.fuzzy_match(&agent.name, query) {
                best = Some(s);
            }
            if let Some(s) = matcher.fuzzy_match(&agent.id, query) {
                best = best.map_or(Some(s), |prev| Some(prev.max(s)));
            }
            // NOTE: Also match "agent <name>" as a compound so typing "agent syn" surfaces the agent.
            let compound = format!("agent {}", agent.name);
            if let Some(s) = matcher.fuzzy_match(&compound, query) {
                best = best.map_or(Some(s), |prev| Some(prev.max(s)));
            }
            if let Some(score) = best {
                suggestions.push(agent_suggestion(agent, score));
            }
        }
    }

    suggestions.sort_by(|a, b| b.score.cmp(&a.score));
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

fn best_match(matcher: &SkimMatcherV2, cmd: &Command, query: &str) -> Option<i64> {
    let mut best: Option<i64> = None;

    if let Some(s) = matcher.fuzzy_match(cmd.name, query) {
        best = Some(s);
    }
    for alias in cmd.aliases {
        if let Some(s) = matcher.fuzzy_match(alias, query) {
            best = best.map_or(Some(s), |prev| Some(prev.max(s)));
        }
    }
    if let Some(s) = matcher.fuzzy_match(cmd.description, query) {
        best = best.map_or(Some(s), |prev| Some(prev.max(s)));
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
            active_tool: None,
            sessions: Vec::new(),
            model: Some("claude-opus-4-6".into()),
            compaction_stage: None,
            distill_completed_at: None,
            unread_count: 0,
            tools: Vec::new(),
        }];
        let results = build_suggestions("syn", &agents);
        assert!(results.iter().any(|r| r.execute_as == "agent syn"));
    }

    #[test]
    fn shortcut_present_on_help() {
        let results = filter_commands("help");
        let help = results.iter().find(|r| r.label == "help").unwrap();
        assert_eq!(help.shortcut, Some("F1"));
    }

    #[test]
    fn sessions_command_has_ctrl_s_shortcut() {
        let results = filter_commands("sessions");
        let cmd = results.iter().find(|r| r.label == "sessions").unwrap();
        assert_eq!(cmd.shortcut, Some("Ctrl+S"));
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
