//! Organon tool limits configuration.

use serde::{Deserialize, Serialize};

/// Organon tool size, timeout, and length limits.
///
/// All defaults match the current hardcoded constants in the `organon` crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct ToolLimitsConfig {
    /// Maximum character length for glob patterns. Default: 1000.
    /// Mirrors `organon::builtins::filesystem::MAX_PATTERN_LENGTH`.
    pub max_pattern_length: usize,
    /// Timeout in seconds for filesystem subprocess commands. Default: 60.
    /// Mirrors `organon::builtins::filesystem::SUBPROCESS_TIMEOUT`.
    pub subprocess_timeout_secs: u64,
    /// Maximum bytes per workspace write operation. Default: 10485760 (10 MiB).
    /// Mirrors `organon::builtins::workspace::MAX_WRITE_BYTES`.
    pub max_write_bytes: usize,
    /// Maximum bytes per workspace read operation. Default: 52428800 (50 MiB).
    /// Mirrors `organon::builtins::workspace::MAX_READ_BYTES`.
    pub max_read_bytes: u64,
    /// Maximum character length of a shell command. Default: 10000.
    /// Mirrors `organon::builtins::workspace::MAX_COMMAND_LENGTH`.
    pub max_command_length: usize,
    /// Maximum characters per intra-session message. Default: 4000.
    /// Mirrors `organon::builtins::communication::MESSAGE_MAX_LEN`.
    pub message_max_len: usize,
    /// Maximum characters per inter-session message. Default: 100000.
    /// Mirrors `organon::builtins::communication::INTER_SESSION_MAX_MESSAGE_LEN`.
    pub inter_session_max_message_len: usize,
    /// Maximum wait timeout in seconds for inter-session messages. Default: 300.
    /// Mirrors `organon::builtins::communication::INTER_SESSION_MAX_TIMEOUT_SECS`.
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
            max_pattern_length: 1_000,
            subprocess_timeout_secs: 60,
            max_write_bytes: 10_485_760,
            max_read_bytes: 52_428_800,
            max_command_length: 10_000,
            message_max_len: 4_000,
            inter_session_max_message_len: 100_000,
            inter_session_max_timeout_secs: 300,
            max_dispatch_tasks: 10,
            agent_dispatch_timeout_secs: 300,
            datalog_default_row_limit: 100,
            datalog_default_timeout_secs: 5.0,
            max_image_bytes: 20_971_520,
            max_pdf_bytes: 33_554_432,
        }
    }
}
