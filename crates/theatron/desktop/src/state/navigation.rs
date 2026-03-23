//! Navigation action state for cross-component communication.
//!
//! Provides a signal-based mechanism for dispatching navigation actions
//! from toast buttons to view components without tight coupling.

/// A navigation action dispatched from a toast or other UI element.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub(crate) enum NavAction {
    /// Open the diff viewer for a specific file path.
    OpenDiff(String),
}

/// Extract a `NavAction` from a toast action_id string.
///
/// Returns `Some(NavAction)` if the action_id encodes a navigation action,
/// `None` otherwise.
#[must_use]
pub(crate) fn parse_action_id(action_id: &str) -> Option<NavAction> {
    action_id
        .strip_prefix("open_diff:")
        .map(|path| NavAction::OpenDiff(path.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_open_diff_action() {
        let action = parse_action_id("open_diff:src/main.rs");
        assert_eq!(action, Some(NavAction::OpenDiff("src/main.rs".to_string())));
    }

    #[test]
    fn returns_none_for_unknown_action() {
        assert_eq!(parse_action_id("unknown:foo"), None);
        assert_eq!(parse_action_id(""), None);
    }
}
