use std::collections::HashMap;

use crate::config::Config;
use crate::id::{NousId, SessionId};

pub(crate) const MAX_COMMAND_HISTORY: usize = 1000;

fn history_file_path(config: &Config) -> Option<std::path::PathBuf> {
    config
        .workspace_root
        .as_ref()
        .map(|root| root.join("state").join("tui_history"))
}

pub(super) fn load_command_history(config: &Config) -> Vec<String> {
    let path = match history_file_path(config) {
        Some(p) => p,
        None => return Vec::new(),
    };
    match std::fs::read_to_string(&path) {
        Ok(contents) => contents
            .lines()
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect(),
        Err(_) => Vec::new(),
    }
}

pub(crate) fn save_command_history(config: &Config, history: &[String]) {
    let path = match history_file_path(config) {
        Some(p) => p,
        None => return,
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let content: String = history.iter().map(|s| format!("{s}\n")).collect();
    #[expect(
        clippy::disallowed_methods,
        reason = "theatron TUI reads configuration and exports from disk in synchronous initialization paths"
    )]
    let _ = std::fs::write(&path, content);
}

fn session_state_file_path(config: &Config) -> Option<std::path::PathBuf> {
    config
        .workspace_root
        .as_ref()
        .map(|root| root.join("state").join("tui_sessions"))
}

/// Load the per-agent last-active session map from disk.
///
/// Format: one entry per line, `<agent_id>:<session_id>`.
/// Malformed or empty lines are silently skipped.
pub(super) fn load_session_state(config: &Config) -> HashMap<NousId, SessionId> {
    let path = match session_state_file_path(config) {
        Some(p) => p,
        None => return HashMap::new(),
    };
    let contents = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return HashMap::new(),
    };
    contents
        .lines()
        .filter_map(|line| {
            let (agent, session) = line.split_once(':')?;
            if agent.is_empty() || session.is_empty() {
                return None;
            }
            Some((NousId::from(agent), SessionId::from(session)))
        })
        .collect()
}

/// Persist the per-agent last-active session map to disk.
///
/// Uses sync I/O because this runs in a synchronous TUI shutdown path
/// where spawning an async task would require a runtime handle that may
/// already be shutting down.
pub(crate) fn save_session_state(config: &Config, sessions: &HashMap<NousId, SessionId>) {
    let path = match session_state_file_path(config) {
        Some(p) => p,
        None => return,
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let content: String = sessions
        .iter()
        .map(|(agent, session)| format!("{agent}:{session}\n"))
        .collect();
    #[expect(
        clippy::disallowed_methods,
        reason = "synchronous write is intentional in TUI shutdown path"
    )]
    let _ = std::fs::write(&path, content);
}

/// Resolve the root directory for export files.
pub(crate) fn exports_dir(config: &Config) -> std::path::PathBuf {
    config
        .workspace_root
        .as_ref()
        .map(|root| root.join("exports"))
        .unwrap_or_else(|| std::path::PathBuf::from("exports"))
}
