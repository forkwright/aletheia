//! Operator credential management DTOs mirroring Pylon's wire shapes
//! (`crates/pylon/src/handlers/credentials.rs`,
//! `crates/pylon/src/credential_runtime.rs`). Skene has no dependency on
//! pylon, so these are independent structs kept in sync by the contract
//! tests in `super::tests`.

use serde::{Deserialize, Serialize};

use koina::secret::SecretString;

/// Effect of a credential-management mutation on the running harness.
///
/// Mirrors `pylon::credential_runtime::CredentialMutationEffect`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialMutationEffect {
    /// The mutation was applied to the live provider chain without restart.
    Applied,
    /// A process restart is required before the running harness will use the
    /// new credential state.
    RestartRequired,
    /// The on-disk state changed; the file-backed credential chain will pick
    /// it up on its next reload interval, but in-memory cached tokens may
    /// still win until then.
    PendingReload,
    /// The provider is registered, but its runtime credential source is not
    /// managed by these endpoints (e.g. env-var auth or a local subprocess).
    NotSupportedByRuntime,
}

/// Outcome of a provider-aware credential validation call.
///
/// Mirrors `pylon::handlers::credentials::CredentialValidationState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialValidationState {
    /// The provider authenticated the credential.
    Accepted,
    /// The provider explicitly rejected the credential.
    Rejected,
    /// The credential is expired according to locally-known metadata.
    Expired,
    /// The stored credential value is empty or otherwise malformed.
    Malformed,
    /// The provider could not be reached; not evidence the key is bad.
    Unreachable,
    /// No live-check strategy exists for this provider; local inspection only.
    Unknown,
}

/// Usage counters backed by authoritative telemetry.
///
/// Omitted from the response (and thus absent here) when no authoritative
/// telemetry source exists for the credential.
#[derive(Debug, Clone, Deserialize)]
#[expect(
    missing_docs,
    reason = "fields mirror pylon's CredentialUsageCounters; self-documenting by name"
)]
pub struct CredentialUsageCounters {
    pub requests_today: u64,
    pub tokens_today: u64,
    pub source: String,
    pub freshness: String,
    pub scope: String,
    pub state: String,
}

/// Secret-safe credential metadata returned by the credential endpoints.
///
/// Mirrors `pylon::handlers::credentials::CredentialResponse`.
#[derive(Debug, Clone, Deserialize)]
pub struct CredentialResponse {
    /// Stable identifier in `{provider}:{role}` form.
    pub id: String,
    /// Provider name associated with the credential.
    pub provider: String,
    /// Role of this credential for its provider.
    pub role: String,
    /// Redacted preview of the credential, never raw secret material.
    #[serde(rename = "masked_key")]
    pub redacted_preview: String,
    /// Effective status: the persisted provider-validation outcome when one
    /// exists, otherwise local-inspection status.
    pub status: String,
    /// `true` when `status` reflects an actual provider round trip.
    pub provider_verified: bool,
    /// The raw persisted provider-validation outcome, when this credential
    /// has ever been validated.
    #[serde(default)]
    pub validation_state: Option<CredentialValidationState>,
    /// Last validation timestamp when produced by a validation call.
    #[serde(default)]
    pub last_validated: Option<String>,
    /// Whether per-credential usage counters are backed by authoritative
    /// provider/session telemetry.
    pub usage_counters_available: bool,
    /// Usage counters when authoritative telemetry is available.
    #[serde(default)]
    pub usage_counters: Option<CredentialUsageCounters>,
    /// Runtime effect of the mutation that produced this credential response.
    #[serde(default)]
    pub runtime_effect: Option<CredentialMutationEffect>,
}

/// Response body for credential list and mutation endpoints.
///
/// Mirrors `pylon::handlers::credentials::CredentialsListResponse`.
#[derive(Debug, Clone, Deserialize)]
pub struct CredentialsListResponse {
    /// Secret-safe credential metadata.
    pub credentials: Vec<CredentialResponse>,
    /// Runtime effect of the mutation on the live provider chain, when this
    /// response was produced by a mutating endpoint (rotate).
    #[serde(default)]
    pub runtime_effect: Option<CredentialMutationEffect>,
}

/// Response body for a credential removal.
///
/// Mirrors `pylon::handlers::credentials::CredentialRemoveResponse`.
#[derive(Debug, Clone, Deserialize)]
pub struct CredentialRemoveResponse {
    /// Runtime effect of the removal on the live provider chain.
    pub runtime_effect: CredentialMutationEffect,
}

/// Request body for adding a provider credential.
///
/// Mirrors `pylon::handlers::credentials::AddCredentialRequest`.
///
/// WHY: `SecretString`'s [`Serialize`] impl always emits the literal string
/// `"[REDACTED]"` (by design, to prevent accidental logging) -- deriving
/// `Serialize` on this struct without a field override would therefore send
/// the literal text `"[REDACTED]"` to pylon as the credential value on every
/// call, not the real key. `serialize_key` opts this one field out of that
/// redaction so the actual secret reaches the wire, the same way
/// `expose_secret()` is the audited escape hatch everywhere else.
#[derive(Clone, Serialize)]
pub struct AddCredentialRequest {
    /// Provider name.
    pub provider: String,
    /// Raw key to store encrypted at rest.
    #[serde(serialize_with = "serialize_key")]
    pub key: SecretString,
    /// Credential role: `primary` or `backup`.
    pub role: String,
}

fn serialize_key<S: serde::Serializer>(
    key: &SecretString,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(key.expose_secret())
}

impl std::fmt::Debug for AddCredentialRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AddCredentialRequest")
            .field("provider", &self.provider)
            .field("key", &"[REDACTED]")
            .field("role", &self.role)
            .finish()
    }
}
