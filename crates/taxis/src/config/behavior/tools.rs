//! Organon tool limits configuration.

use serde::{Deserialize, Serialize};
/// Default value used for `ToolLimitsConfig::max_pattern_length`.
pub(crate) const DEFAULT_MAX_PATTERN_LENGTH: usize = 1_000;
/// Default value used for `ToolLimitsConfig::subprocess_timeout_secs`.
pub(crate) const DEFAULT_SUBPROCESS_TIMEOUT_SECS: u64 = 60;
/// Default value used for `ToolLimitsConfig::max_write_bytes`.
pub(crate) const DEFAULT_MAX_WRITE_BYTES: usize = 10_485_760;
/// Default value used for `ToolLimitsConfig::max_read_bytes`.
pub(crate) const DEFAULT_MAX_READ_BYTES: u64 = 52_428_800;
/// Default value used for `ToolLimitsConfig::max_command_length`.
pub(crate) const DEFAULT_MAX_COMMAND_LENGTH: usize = 10_000;
/// Default value used for `ToolLimitsConfig::message_max_len`.
pub(crate) const DEFAULT_MESSAGE_MAX_LEN: usize = 4_000;
/// Default value used for `ToolLimitsConfig::inter_session_max_message_len`.
pub(crate) const DEFAULT_INTER_SESSION_MAX_MESSAGE_LEN: usize = 100_000;
/// Default value used for `ToolLimitsConfig::inter_session_max_timeout_secs`.
pub(crate) const DEFAULT_INTER_SESSION_MAX_TIMEOUT_SECS: u64 = 300;

/// Organon tool size, timeout, and length limits.
///
/// Defaults for the fields that mirror `organon` constants are enforced at
/// test-build time by `const _: () = assert!` guards below.

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct ToolLimitsConfig {
    /// Maximum character length for glob patterns.
    pub max_pattern_length: usize,
    /// Timeout in seconds for filesystem subprocess commands.
    pub subprocess_timeout_secs: u64,
    /// Maximum bytes per workspace write operation.
    pub max_write_bytes: usize,
    /// Maximum bytes per workspace read operation.
    pub max_read_bytes: u64,
    /// Maximum character length of a shell command.
    pub max_command_length: usize,
    /// Maximum characters per intra-session message.
    pub message_max_len: usize,
    /// Maximum characters per inter-session message.
    pub inter_session_max_message_len: usize,
    /// Maximum wait timeout in seconds for inter-session messages.
    pub inter_session_max_timeout_secs: u64,
    /// Maximum concurrent agent-dispatch tasks. Default: 10.
    /// Also present in `AgentBehaviorDefaults::tool_agent_dispatch_max_tasks`.
    pub max_dispatch_tasks: usize,
    /// Default timeout in seconds for spawned sub-agents. Default: 300.
    pub agent_dispatch_timeout_secs: u64,
    /// Default row limit for Datalog memory queries. Default: 100.
    /// Also present in `AgentBehaviorDefaults::tool_datalog_default_row_limit`.
    pub datalog_default_row_limit: usize,
    /// Default query timeout in seconds for the Datalog memory tool. Default: 5.0.
    /// Also present in `AgentBehaviorDefaults::tool_datalog_default_timeout_secs`.
    pub datalog_default_timeout_secs: f64,
    /// Maximum image file size in bytes for the view-file tool. Default: 20971520 (20 MiB).
    /// Also present in `AgentBehaviorDefaults::tool_max_image_bytes`.
    pub max_image_bytes: u64,
    /// Maximum PDF file size in bytes for the view-file tool. Default: 33554432 (32 MiB).
    /// Also present in `AgentBehaviorDefaults::tool_max_pdf_bytes`.
    pub max_pdf_bytes: u64,
}

impl Default for ToolLimitsConfig {
    fn default() -> Self {
        Self {
            max_pattern_length: DEFAULT_MAX_PATTERN_LENGTH,
            subprocess_timeout_secs: DEFAULT_SUBPROCESS_TIMEOUT_SECS,
            max_write_bytes: DEFAULT_MAX_WRITE_BYTES,
            max_read_bytes: DEFAULT_MAX_READ_BYTES,
            max_command_length: DEFAULT_MAX_COMMAND_LENGTH,
            message_max_len: DEFAULT_MESSAGE_MAX_LEN,
            inter_session_max_message_len: DEFAULT_INTER_SESSION_MAX_MESSAGE_LEN,
            inter_session_max_timeout_secs: DEFAULT_INTER_SESSION_MAX_TIMEOUT_SECS,
            max_dispatch_tasks: 10,
            agent_dispatch_timeout_secs: 300,
            datalog_default_row_limit: 100,
            datalog_default_timeout_secs: 5.0,
            max_image_bytes: 20_971_520,
            max_pdf_bytes: 33_554_432,
        }
    }
}

/// Availability of Anthropic server-side tools (web search, code execution)
/// for per-session activation via organon's `enable_tool` meta-tool.
///
/// WHY configurable: server tools run on the provider's infrastructure
/// (Anthropic-hosted web search / code execution), so enabling one carries
/// cost and data-exposure tradeoffs the operator must opt into per
/// deployment. Disabled by default — an absent or default section never
/// implies opt-in, matching `RecallSourcesConfig`'s network-source policy.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct ServerToolsConfig {
    /// Whether web search is available for activation.
    pub web_search: bool,
    /// Maximum web search uses per turn (`None` = provider default).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web_search_max_uses: Option<u32>,
    /// Whether code execution is available for activation.
    pub code_execution: bool,
    /// Provider `tool_type` version strings for each server tool.
    pub versions: ServerToolVersions,
}

/// Provider-assigned version identifiers for Anthropic server-side tools.
///
/// WHY configurable: Anthropic revs `tool_type` version suffixes (e.g.
/// `web_search_20250305`) on its own schedule, independent of aletheia's
/// release cycle. Pinning them here instead of as source literals lets an
/// operator move to a newer or pinned-older revision without a rebuild, and
/// gives this the single place validation checks the value against
/// currently-supported versions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct ServerToolVersions {
    /// `tool_type` string sent for the web-search server tool.
    pub web_search_type: String,
    /// `tool_type` string sent for the code-execution server tool.
    pub code_execution_type: String,
}

impl Default for ServerToolVersions {
    fn default() -> Self {
        Self {
            web_search_type: "web_search_20250305".to_owned(),
            code_execution_type: "code_execution_20250522".to_owned(),
        }
    }
}

impl ServerToolVersions {
    /// Check whether the configured `tool_type` strings look like real
    /// Anthropic server-tool versions, independent of whether server tools
    /// are actually enabled.
    ///
    /// ARCHITECTURE(#4846): every currently-known Anthropic server-tool
    /// `tool_type` follows `<name>_<8-digit-date>` (e.g.
    /// `web_search_20250305`). This cannot validate against a live
    /// allowlist of provider-supported versions -- Anthropic revs these
    /// independently of aletheia's release cycle, and hardcoding a
    /// snapshot of "currently valid" values would itself drift the moment
    /// a new one ships. What IS checkable without a network call is
    /// shape: an operator-mistyped or stale value (empty string, missing
    /// the date suffix, non-digit suffix) is almost certainly wrong
    /// regardless of which specific dates are current, and this is the
    /// class of error a config typo actually produces.
    #[must_use]
    pub fn validate(&self) -> Vec<String> {
        let mut issues = Vec::new();
        for (field, value) in [
            ("serverTools.versions.webSearchType", &self.web_search_type),
            (
                "serverTools.versions.codeExecutionType",
                &self.code_execution_type,
            ),
        ] {
            if let Some(reason) = Self::shape_issue(value) {
                issues.push(format!(
                    "{field} = {value:?} does not look like an Anthropic server-tool version \
                     string ({reason}); expected the form \"<name>_<8-digit-date>\", e.g. \
                     \"web_search_20250305\""
                ));
            }
        }
        issues
    }

    /// Returns `Some(reason)` when `value` does not match the
    /// `<name>_<8-digit-date>` shape every known Anthropic server-tool
    /// `tool_type` follows.
    fn shape_issue(value: &str) -> Option<&'static str> {
        if value.is_empty() {
            return Some("empty");
        }
        let Some((name, date)) = value.rsplit_once('_') else {
            return Some("missing a `_<date>` suffix");
        };
        if name.is_empty() {
            return Some("empty name before the date suffix");
        }
        if date.len() != 8 || !date.bytes().all(|b| b.is_ascii_digit()) {
            return Some("date suffix is not exactly 8 digits");
        }
        None
    }
}

#[cfg(test)]
const _: () =
    assert!(DEFAULT_MAX_PATTERN_LENGTH == organon::builtins::filesystem::MAX_PATTERN_LENGTH);
#[cfg(test)]
const _: () = assert!(
    DEFAULT_SUBPROCESS_TIMEOUT_SECS == organon::builtins::filesystem::SUBPROCESS_TIMEOUT.as_secs()
);
#[cfg(test)]
const _: () = assert!(DEFAULT_MAX_WRITE_BYTES == organon::builtins::workspace::MAX_WRITE_BYTES);
#[cfg(test)]
const _: () = assert!(DEFAULT_MAX_READ_BYTES == organon::builtins::workspace::MAX_READ_BYTES);
#[cfg(test)]
const _: () =
    assert!(DEFAULT_MAX_COMMAND_LENGTH == organon::builtins::workspace::MAX_COMMAND_LENGTH);
#[cfg(test)]
const _: () = assert!(DEFAULT_MESSAGE_MAX_LEN == organon::builtins::communication::MESSAGE_MAX_LEN);
#[cfg(test)]
const _: () = assert!(
    DEFAULT_INTER_SESSION_MAX_MESSAGE_LEN
        == organon::builtins::communication::INTER_SESSION_MAX_MESSAGE_LEN
);
#[cfg(test)]
const _: () = assert!(
    DEFAULT_INTER_SESSION_MAX_TIMEOUT_SECS
        == organon::builtins::communication::INTER_SESSION_MAX_TIMEOUT_SECS
);

// WHY this module is last: clippy::items_after_test_module forbids any item
// (including the const _: () assertions above, which predate this module)
// appearing textually after a #[cfg(test)] mod -- so the test module must
// be the final item in the file.
#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod server_tool_versions_tests {
    use super::ServerToolVersions;

    #[test]
    fn default_versions_pass_validation() {
        assert!(
            ServerToolVersions::default().validate().is_empty(),
            "the compiled default must itself be shape-valid"
        );
    }

    #[test]
    fn empty_version_is_flagged() {
        let versions = ServerToolVersions {
            web_search_type: String::new(),
            ..ServerToolVersions::default()
        };
        let issues = versions.validate();
        assert_eq!(issues.len(), 1, "exactly the empty field must be flagged");
        assert!(
            issues
                .first()
                .expect("length asserted above")
                .contains("webSearchType")
        );
    }

    #[test]
    fn missing_date_suffix_is_flagged() {
        let versions = ServerToolVersions {
            code_execution_type: "code_execution".to_owned(),
            ..ServerToolVersions::default()
        };
        let issues = versions.validate();
        assert_eq!(issues.len(), 1);
        assert!(
            issues
                .first()
                .expect("length asserted above")
                .contains("codeExecutionType")
        );
    }

    #[test]
    fn non_digit_date_suffix_is_flagged() {
        let versions = ServerToolVersions {
            web_search_type: "web_search_notadate".to_owned(),
            ..ServerToolVersions::default()
        };
        assert_eq!(versions.validate().len(), 1);
    }

    #[test]
    fn well_formed_custom_version_passes() {
        // WHY: a real future revision (e.g. Anthropic ships
        // web_search_20260101) must validate cleanly -- this is a shape
        // check, not an allowlist of today's known-good values.
        let versions = ServerToolVersions {
            web_search_type: "web_search_20260101".to_owned(),
            ..ServerToolVersions::default()
        };
        assert!(versions.validate().is_empty());
    }
}
