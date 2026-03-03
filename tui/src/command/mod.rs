/// Command registry and fuzzy matching for the `:` command palette.
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}

#[derive(Debug)]
pub struct ScoredCommand {
    pub index: usize,
    pub score: i64,
}

pub static COMMANDS: &[Command] = &[
    Command {
        name: "sessions",
        aliases: &["s"],
        description: "List sessions for current agent",
        category: CommandCategory::Navigation,
    },
    Command {
        name: "agents",
        aliases: &["a"],
        description: "Switch agent",
        category: CommandCategory::Navigation,
    },
    Command {
        name: "agent",
        aliases: &[],
        description: "Switch to named agent",
        category: CommandCategory::Agent,
    },
    Command {
        name: "cost",
        aliases: &["$"],
        description: "Show daily cost breakdown",
        category: CommandCategory::Query,
    },
    Command {
        name: "health",
        aliases: &["h"],
        description: "System health status",
        category: CommandCategory::Query,
    },
    Command {
        name: "compact",
        aliases: &[],
        description: "Trigger distillation",
        category: CommandCategory::Action,
    },
    Command {
        name: "clear",
        aliases: &[],
        description: "Clear conversation / new session",
        category: CommandCategory::Action,
    },
    Command {
        name: "help",
        aliases: &["?"],
        description: "Show help",
        category: CommandCategory::Navigation,
    },
    Command {
        name: "quit",
        aliases: &["q"],
        description: "Quit application",
        category: CommandCategory::Action,
    },
    Command {
        name: "recall",
        aliases: &["r"],
        description: "Search memory graph",
        category: CommandCategory::Query,
    },
    Command {
        name: "model",
        aliases: &[],
        description: "Show current model info",
        category: CommandCategory::Query,
    },
];

const MAX_SUGGESTIONS: usize = 8;

/// Filter and rank commands by fuzzy matching against the input.
///
/// When input contains a space, only the first word is matched against commands.
/// Returns up to 8 results sorted by score descending.
pub fn filter_commands(input: &str) -> Vec<ScoredCommand> {
    let query = match input.split_once(' ') {
        Some((cmd, _)) => cmd,
        None => input,
    };

    if query.is_empty() {
        return COMMANDS
            .iter()
            .enumerate()
            .map(|(i, _)| ScoredCommand { index: i, score: 0 })
            .collect();
    }

    let matcher = SkimMatcherV2::default();
    let mut scored: Vec<ScoredCommand> = COMMANDS
        .iter()
        .enumerate()
        .filter_map(|(i, cmd)| {
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

            best.map(|score| ScoredCommand { index: i, score })
        })
        .collect();

    scored.sort_by(|a, b| b.score.cmp(&a.score));
    scored.truncate(MAX_SUGGESTIONS);
    scored
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_returns_all_commands() {
        let results = filter_commands("");
        assert_eq!(results.len(), COMMANDS.len());
    }

    #[test]
    fn exact_name_match_ranks_first() {
        let results = filter_commands("quit");
        assert!(!results.is_empty());
        assert_eq!(COMMANDS[results[0].index].name, "quit");
    }

    #[test]
    fn alias_match_works() {
        let results = filter_commands("q");
        assert!(!results.is_empty());
        assert!(results.iter().any(|r| COMMANDS[r.index].name == "quit"));
    }

    #[test]
    fn fuzzy_match_partial() {
        let results = filter_commands("sess");
        assert!(!results.is_empty());
        assert_eq!(COMMANDS[results[0].index].name, "sessions");
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
        assert!(results.iter().any(|r| COMMANDS[r.index].name == "agent"));
    }
}
