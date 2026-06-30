//! Credential management state for the ops view.

const MASKED_KEY_PREFIX: &str = "...";
const MASKED_KEY_PLACEHOLDER: &str = "...????";
const MIN_SECRET_PREVIEW_CHARS: usize = 9;
const SAFE_PREVIEW_CHARS: usize = 4;

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
            Self::Valid => "var(--status-success)",
            Self::Expired => "var(--status-error)",
            Self::Untested => "var(--text-secondary)",
        }
    }
}

/// A single credential entry.
///
/// NOTE: `masked_key` contains either a placeholder or only the last 4
/// characters of a validated long key prefixed with "...". Full key values must
/// never be stored here.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CredentialEntry {
    // kanon:ignore RUST/primitive-for-domain-id — CredentialEntry id mirrors the external provider string identifier
    pub(crate) id: String,
    pub(crate) provider: String,
    pub(crate) role: CredentialRole,
    /// Display form of the key: placeholder or last 4 chars prefixed with "...".
    pub(crate) masked_key: String, // kanon:ignore RUST/plain-string-secret -- masked display suffix only, never the raw credential (#3988)
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

    /// Distinct provider names in insertion order.
    #[cfg_attr(not(test), expect(dead_code, reason = "used in tests"))]
    #[must_use]
    pub(crate) fn providers(&self) -> Vec<&str> {
        let mut seen = Vec::new();
        for entry in &self.entries {
            if !seen.contains(&entry.provider.as_str()) {
                seen.push(entry.provider.as_str());
            }
        }
        seen
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
}

/// Masks a credential key to show only a safe preview.
///
/// Returns `"...XXXX"` where XXXX is the last 4 chars of a long `key`.
/// Returns `"...????"` if `key` is too short to preview without potentially
/// revealing the whole credential.
pub(crate) fn mask_key(key: &str) -> String {
    if key.chars().count() < MIN_SECRET_PREVIEW_CHARS {
        return MASKED_KEY_PLACEHOLDER.to_string();
    }
    let tail: String = key
        .chars()
        .rev()
        .take(SAFE_PREVIEW_CHARS)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{MASKED_KEY_PREFIX}{tail}")
}

/// Canonicalize a masked key received from the API before it enters UI state.
///
/// WHY(#4876): a server bug could return `...raw-secret-material`; prefix
/// checks would treat that as safe and preserve almost the whole secret.
/// Preserve only already-canonical previews, mask unprefixed raw values, and
/// collapse malformed prefixed values to a non-secret placeholder.
pub(crate) fn canonicalize_masked_key(api_value: &str) -> String {
    if is_canonical_masked_key(api_value) {
        return api_value.to_owned();
    }
    if api_value.starts_with(MASKED_KEY_PREFIX) {
        return MASKED_KEY_PLACEHOLDER.to_string();
    }
    mask_key(api_value)
}

fn is_canonical_masked_key(value: &str) -> bool {
    value.starts_with(MASKED_KEY_PREFIX)
        && value
            .strip_prefix(MASKED_KEY_PREFIX)
            .is_some_and(|suffix| suffix.chars().count() == SAFE_PREVIEW_CHARS)
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
        assert_eq!(mask_key("ab12"), "...????");
    }

    #[test]
    fn mask_key_too_short_returns_placeholder() {
        assert_eq!(mask_key("abc"), "...????");
    }

    #[test]
    fn mask_key_empty_returns_placeholder() {
        assert_eq!(mask_key(""), "...????");
    }

    #[test]
    fn mask_key_handles_unicode_char_boundaries() {
        assert_eq!(mask_key("sk-long-αβγδ"), "...αβγδ");
    }

    #[test]
    fn mask_key_hides_all_short_inputs() {
        for len in 1..=8 {
            let raw = "a".repeat(len);

            let masked = mask_key(&raw);

            assert_eq!(masked, MASKED_KEY_PLACEHOLDER);
            assert!(!masked.contains(&raw));
        }
    }

    #[test]
    fn canonicalize_masked_key_preserves_canonical_preview() {
        assert_eq!(canonicalize_masked_key("...ab12"), "...ab12");
    }

    #[test]
    fn canonicalize_masked_key_masks_unprefixed_raw_key() {
        assert_eq!(canonicalize_masked_key("sk-test-secret-1234"), "...1234");
    }

    #[test]
    fn canonicalize_masked_key_collapses_malformed_prefixed_payload() {
        assert_eq!(
            canonicalize_masked_key("...raw-secret-material"),
            MASKED_KEY_PLACEHOLDER
        );
    }
}
