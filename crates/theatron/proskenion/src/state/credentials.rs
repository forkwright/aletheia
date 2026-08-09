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

/// Validation status of a credential, as surfaced by the last check.
///
/// WHY(#4875): this alone never distinguishes a real provider round trip from
/// local-only inspection (does the file load, has it not locally expired) —
/// pair it with `CredentialEntry::provider_verified` for that distinction.
/// Previously this type collapsed everything but a bare "valid"/"expired"
/// string into `Untested`, which meant an explicit provider *rejection*
/// rendered identically to "never checked".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub(crate) enum ValidationStatus {
    /// Accepted: the file loads and, when checked, the provider authenticated it.
    Valid,
    /// The provider explicitly rejected the credential.
    Invalid,
    /// Locally known expired (kept distinct from an explicit rejection).
    Expired,
    /// The stored credential value is empty or otherwise malformed.
    Malformed,
    /// The provider could not be reached; not evidence the key is bad.
    Unreachable,
    /// Never validated, or no live-check exists for this provider.
    Untested,
}

impl ValidationStatus {
    /// Parse the wire `status` string returned by `GET`/`POST` credential
    /// endpoints. Unrecognized values fall back to `Untested` rather than
    /// panicking or guessing — a forward-compatible default for any future
    /// server-side status this client doesn't know about yet.
    pub(crate) fn from_wire(status: &str) -> Self {
        match status {
            "valid" => Self::Valid,
            "invalid" => Self::Invalid,
            "expired" => Self::Expired,
            "malformed" => Self::Malformed,
            "unreachable" => Self::Unreachable,
            _ => Self::Untested,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Valid => "Valid",
            Self::Invalid => "Invalid",
            Self::Expired => "Expired",
            Self::Malformed => "Malformed",
            Self::Unreachable => "Unreachable",
            Self::Untested => "Untested",
        }
    }

    pub(crate) fn color(self) -> &'static str {
        match self {
            Self::Valid => "var(--status-success)",
            Self::Invalid | Self::Expired | Self::Malformed => "var(--status-error)",
            Self::Unreachable => "var(--status-warning)",
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
    /// `true` when `status` reflects an actual provider round trip, `false`
    /// when it is local inspection only (file loads, not locally expired).
    ///
    /// WHY(#4875): the card must never present "Valid" as though the
    /// provider confirmed it when nobody has ever checked.
    pub(crate) provider_verified: bool,
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

/// Extract the RBAC `role` claim from a locally-held JWT access token,
/// without verifying its signature.
///
/// WHY(#4877): the credentials panel must know whether the caller can
/// actually perform credential mutations before rendering controls the
/// caller cannot use. Reading the claim client-side is a UI-affordance
/// optimization only — the server remains the sole enforcement authority
/// (every mutation is re-checked there via a signature-verified token); a
/// forged or stale claim read here only ever hides a control the server
/// would reject anyway, never grants one the server would refuse.
///
/// Returns `None` when the token is missing, malformed, or carries no
/// readable `role` claim — callers must treat `None` as "cannot manage".
pub(crate) fn decode_role_claim(token: &str) -> Option<String> {
    let payload_b64 = token.split('.').nth(1)?;
    let payload = koina::base64::decode_url_safe_no_pad(payload_b64).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&payload).ok()?;
    value
        .get("role")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}

/// Whether an RBAC role (as decoded by [`decode_role_claim`]) may add,
/// validate, rotate, or remove credentials.
///
/// WHY: mirrors `symbolon::auth::is_authorized`'s `Action::ManageCredentials`
/// rule (`Role::Admin | Role::Operator => true`, everything else `false`).
/// Kept as a narrow, explicit allowlist rather than "not readonly/agent" so
/// a future role added on the server defaults to denied here until this is
/// updated deliberately.
#[must_use]
pub(crate) fn can_manage_credentials(role: Option<&str>) -> bool {
    matches!(role, Some("admin" | "operator"))
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
            provider_verified: false,
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

    // ── ValidationStatus::from_wire ──

    #[test]
    fn validation_status_from_wire_parses_every_known_state() {
        assert_eq!(ValidationStatus::from_wire("valid"), ValidationStatus::Valid);
        assert_eq!(
            ValidationStatus::from_wire("invalid"),
            ValidationStatus::Invalid
        );
        assert_eq!(
            ValidationStatus::from_wire("expired"),
            ValidationStatus::Expired
        );
        assert_eq!(
            ValidationStatus::from_wire("malformed"),
            ValidationStatus::Malformed
        );
        assert_eq!(
            ValidationStatus::from_wire("unreachable"),
            ValidationStatus::Unreachable
        );
    }

    #[test]
    fn validation_status_from_wire_unknown_falls_back_to_untested() {
        assert_eq!(
            ValidationStatus::from_wire("untested"),
            ValidationStatus::Untested
        );
        assert_eq!(
            ValidationStatus::from_wire("some-future-state"),
            ValidationStatus::Untested,
            "an unrecognized status must degrade to Untested, never panic or guess"
        );
        assert_eq!(ValidationStatus::from_wire(""), ValidationStatus::Untested);
    }

    // ── decode_role_claim / can_manage_credentials ──

    fn fake_jwt_with_role(role: &str) -> String {
        let payload = serde_json::json!({"role": role, "sub": "test"});
        let payload_b64 =
            koina::base64::encode_url_safe_no_pad(payload.to_string().as_bytes());
        format!("header.{payload_b64}.signature")
    }

    #[test]
    fn decode_role_claim_reads_operator() {
        let token = fake_jwt_with_role("operator");
        assert_eq!(decode_role_claim(&token).as_deref(), Some("operator"));
    }

    #[test]
    fn decode_role_claim_reads_admin() {
        let token = fake_jwt_with_role("admin");
        assert_eq!(decode_role_claim(&token).as_deref(), Some("admin"));
    }

    #[test]
    fn decode_role_claim_reads_readonly() {
        let token = fake_jwt_with_role("readonly");
        assert_eq!(decode_role_claim(&token).as_deref(), Some("readonly"));
    }

    #[test]
    fn decode_role_claim_none_for_empty_string() {
        assert_eq!(decode_role_claim(""), None);
    }

    #[test]
    fn decode_role_claim_none_for_single_segment() {
        assert_eq!(decode_role_claim("not-a-jwt"), None);
    }

    #[test]
    fn decode_role_claim_none_for_non_base64_payload() {
        assert_eq!(decode_role_claim("header.not!!valid!!base64.sig"), None);
    }

    #[test]
    fn decode_role_claim_none_for_valid_base64_non_json_payload() {
        let payload_b64 = koina::base64::encode_url_safe_no_pad(b"not json at all");
        let token = format!("header.{payload_b64}.sig");
        assert_eq!(decode_role_claim(&token), None);
    }

    #[test]
    fn decode_role_claim_none_when_role_claim_absent() {
        let payload = serde_json::json!({"sub": "test"});
        let payload_b64 =
            koina::base64::encode_url_safe_no_pad(payload.to_string().as_bytes());
        let token = format!("header.{payload_b64}.sig");
        assert_eq!(decode_role_claim(&token), None);
    }

    #[test]
    fn can_manage_credentials_true_for_operator_and_admin() {
        assert!(can_manage_credentials(Some("operator")));
        assert!(can_manage_credentials(Some("admin")));
    }

    #[test]
    fn can_manage_credentials_false_for_readonly_agent_none_or_unknown() {
        assert!(!can_manage_credentials(Some("readonly")));
        assert!(!can_manage_credentials(Some("agent")));
        assert!(!can_manage_credentials(Some("Operator")));
        assert!(!can_manage_credentials(Some("")));
        assert!(!can_manage_credentials(None));
    }
}
