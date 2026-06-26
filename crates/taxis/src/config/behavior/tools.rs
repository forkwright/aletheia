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
