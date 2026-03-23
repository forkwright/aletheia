//! Credential management state for the ops view.

use std::collections::HashSet;

/// Role of a credential relative to its provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub(crate) enum CredentialRole {
    /// Active credential used for API calls.
    Primary,
    /// Standby credential used when primary is unavailable.
    Backup,
}

impl CredentialRole {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Primary => "Primary",
            Self::Backup => "Backup",
        }
    }
}

/// Validation status of a credential.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub(crate) enum ValidationStatus {
    /// Credential tested and accepted by provider.
    Valid,
    /// Credential tested and rejected by provider.
    Expired,
    /// Credential has not been tested.
    Untested,
}

impl ValidationStatus {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Valid => "Valid",
            Self::Expired => "Expired",
            Self::Untested => "Untested",
        }
    }

    pub(crate) fn color(self) -> &'static str {
        match self {
            Self::Valid => "#22c55e",
            Self::Expired => "#ef4444",
            Self::Untested => "#888888",
        }
    }
}

/// A single credential entry.
///
/// NOTE: `masked_key` contains only the last 4 characters of the key prefixed
/// with "..." (e.g., `"...ab12"`). Full key values must never be stored here.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CredentialEntry {
    pub(crate) id: String,
    pub(crate) provider: String,
    pub(crate) role: CredentialRole,
    /// Display form of the key: last 4 chars prefixed with "...".
    pub(crate) masked_key: String,
    pub(crate) status: ValidationStatus,
    pub(crate) last_validated: Option<String>,
    pub(crate) requests_today: u64,
    pub(crate) tokens_today: u64,
}

/// Store for credential entries.
#[derive(Debug, Clone, Default)]
pub(crate) struct CredentialStore {
    pub(crate) entries: Vec<CredentialEntry>,
}

impl CredentialStore {
    /// Returns true if removing the credential with `id` would leave its
    /// provider with no primary credential.
    #[must_use]
    pub(crate) fn is_last_primary(&self, id: &str) -> bool {
        let Some(entry) = self.entries.iter().find(|e| e.id == id) else {
            return false;
        };
        if entry.role != CredentialRole::Primary {
            return false;
        }
        let provider = &entry.provider;
        self.entries
            .iter()
            .filter(|e| &e.provider == provider && e.role == CredentialRole::Primary)
            .count()
            == 1
    }

    /// Returns true if the provider has both a primary and a backup credential.
    #[must_use]
    pub(crate) fn can_rotate(&self, provider: &str) -> bool {
        let has_primary = self
            .entries
            .iter()
            .any(|e| e.provider == provider && e.role == CredentialRole::Primary);
        let has_backup = self
            .entries
            .iter()
            .any(|e| e.provider == provider && e.role == CredentialRole::Backup);
        has_primary && has_backup
    }

    /// Providers present in the store (deduplicated, stable insertion order).
    #[must_use]
    pub(crate) fn providers(&self) -> Vec<&str> {
        let mut seen = HashSet::new();
        self.entries
            .iter()
            .filter_map(|e| {
                if seen.insert(e.provider.as_str()) {
                    Some(e.provider.as_str())
                } else {
                    None
                }
            })
            .collect()
    }
}

/// Masks a credential key to show only the last 4 characters.
///
/// Returns `"...XXXX"` where XXXX is the last 4 chars of `key`.
/// Returns `"...????"` if `key` is shorter than 4 characters.
pub(crate) fn mask_key(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() >= 4 {
        let tail: String = chars[chars.len() - 4..].iter().collect();
        format!("...{tail}")
    } else {
        "...????".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, provider: &str, role: CredentialRole) -> CredentialEntry {
        CredentialEntry {
            id: id.to_string(),
            provider: provider.to_string(),
            role,
            masked_key: "...ab12".to_string(),
            status: ValidationStatus::Untested,
            last_validated: None,
            requests_today: 0,
            tokens_today: 0,
        }
    }

    fn store(entries: Vec<CredentialEntry>) -> CredentialStore {
        CredentialStore { entries }
    }

    #[test]
    fn is_last_primary_true_when_single_primary() {
        let s = store(vec![entry("c1", "anthropic", CredentialRole::Primary)]);
        assert!(s.is_last_primary("c1"));
    }

    #[test]
    fn is_last_primary_false_when_backup() {
        let s = store(vec![entry("c1", "anthropic", CredentialRole::Backup)]);
        assert!(!s.is_last_primary("c1"));
    }

    #[test]
    fn is_last_primary_false_when_two_primaries() {
        let s = store(vec![
            entry("c1", "anthropic", CredentialRole::Primary),
            entry("c2", "anthropic", CredentialRole::Primary),
        ]);
        assert!(!s.is_last_primary("c1"));
    }

    #[test]
    fn is_last_primary_false_for_unknown_id() {
        let s = store(vec![entry("c1", "anthropic", CredentialRole::Primary)]);
        assert!(!s.is_last_primary("missing"));
    }

    #[test]
    fn can_rotate_true_with_primary_and_backup() {
        let s = store(vec![
            entry("c1", "anthropic", CredentialRole::Primary),
            entry("c2", "anthropic", CredentialRole::Backup),
        ]);
        assert!(s.can_rotate("anthropic"));
    }

    #[test]
    fn can_rotate_false_with_only_primary() {
        let s = store(vec![entry("c1", "anthropic", CredentialRole::Primary)]);
        assert!(!s.can_rotate("anthropic"));
    }

    #[test]
    fn can_rotate_false_for_unknown_provider() {
        let s = store(vec![entry("c1", "anthropic", CredentialRole::Primary)]);
        assert!(!s.can_rotate("openai"));
    }

    #[test]
    fn providers_deduplicates_stable_order() {
        let s = store(vec![
            entry("c1", "anthropic", CredentialRole::Primary),
            entry("c2", "anthropic", CredentialRole::Backup),
            entry("c3", "openai", CredentialRole::Primary),
        ]);
        let providers = s.providers();
        assert_eq!(providers, vec!["anthropic", "openai"]);
    }

    #[test]
    fn mask_key_last_four_chars() {
        assert_eq!(mask_key("sk-abc123def456"), "...f456");
    }

    #[test]
    fn mask_key_exactly_four_chars() {
        assert_eq!(mask_key("ab12"), "...ab12");
    }

    #[test]
    fn mask_key_too_short_returns_placeholder() {
        assert_eq!(mask_key("abc"), "...????");
    }

    #[test]
    fn mask_key_empty_returns_placeholder() {
        assert_eq!(mask_key(""), "...????");
    }
}
