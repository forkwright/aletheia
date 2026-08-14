//! Network-backed recall source configuration.

use serde::{Deserialize, Serialize};

/// External recall sources merged into the knowledge pipeline by
/// [`crate::config::AletheiaConfig::recall_sources`].
///
/// SECURITY(#6444): every source defaults to disabled. `memory_search` is
/// classified and auto-activated as a local read tool; without an explicit
/// entry here, registering a network-backed source would send the operator's
/// raw query to a third party with no opt-in. Enabling a source is a
/// deliberate config change, never a side effect of an API key being present.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct RecallSourcesConfig {
    /// Semantic Scholar Academic Graph API (`api.semanticscholar.org`).
    pub academic: AcademicSourceConfig,
}

/// Configuration for the Semantic Scholar recall source.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct AcademicSourceConfig {
    /// Explicit opt-in. Default `false`: `memory_search` never reaches
    /// `api.semanticscholar.org` unless an operator sets this to `true`, with
    /// or without `SEMANTIC_SCHOLAR_API_KEY` set.
    pub enabled: bool,
}
