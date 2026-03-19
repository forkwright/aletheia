//! Maintenance, monitoring, sandbox, logging, and MCP configuration types.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{EgressPolicy, SandboxEnforcementMode};

// TODO(#1129): Wire retention policy fields when the executor is implemented.
/// Data retention policy configuration (reserved for future use).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct RetentionConfig {}
/// Instance maintenance settings.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct MaintenanceSettings {
    /// Trace log file rotation and compression.
    pub trace_rotation: TraceRotationSettings,
    /// Filesystem drift detection against expected instance layout.
    pub drift_detection: DriftDetectionSettings,
    /// Database size monitoring and alerting.
    pub db_monitoring: DbMonitoringSettings,
    /// Proactive disk space monitoring and write protection.
    pub disk_space: DiskSpaceSettings,
    /// `SQLite` corruption recovery settings.
    pub sqlite_recovery: SqliteRecoverySettings,
    /// Automatic data retention enforcement.
    pub retention: RetentionSettings,
    /// Whether background knowledge graph maintenance tasks are enabled.
    #[serde(default)]
    pub knowledge_maintenance_enabled: bool,
}

/// Trace file rotation settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct TraceRotationSettings {
    /// Whether automatic trace rotation runs.
    pub enabled: bool,
    /// Delete trace files older than this many days.
    pub max_age_days: u32,
    /// Maximum total trace directory size in MB before pruning.
    pub max_total_size_mb: u64,
    /// Whether to gzip-compress rotated trace files.
    pub compress: bool,
    /// Maximum number of compressed archive files to retain.
    pub max_archives: usize,
}

impl Default for TraceRotationSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            max_age_days: 14,
            max_total_size_mb: 500,
            compress: true,
            max_archives: 30,
        }
    }
}

/// Instance drift detection settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct DriftDetectionSettings {
    /// Whether drift detection runs during maintenance.
    pub enabled: bool,
    /// Emit warnings for files missing from the expected layout.
    pub alert_on_missing: bool,
    /// Glob patterns for paths to ignore during drift checks entirely.
    pub ignore_patterns: Vec<String>,
    /// Glob patterns for optional scaffolding files. Missing files matching these
    /// patterns are reported at info level rather than warn level.
    pub optional_patterns: Vec<String>,
}

impl Default for DriftDetectionSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            alert_on_missing: true,
            ignore_patterns: vec![
                "data/".to_owned(),
                "signal/".to_owned(),
                "*.db".to_owned(),
                ".gitkeep".to_owned(),
            ],
            optional_patterns: vec![
                // WHY: _default/ and _template/ are scaffolding directories that
                // live in the example but are not expected in a live instance
                // (init writes agent files into nous/{id}/ instead).
                "nous/_default/".to_owned(),
                "nous/_template/".to_owned(),
                "packs/".to_owned(),
                "services/".to_owned(),
                "shared/".to_owned(),
                "theke/".to_owned(),
                "logs/".to_owned(),
                "README.md".to_owned(),
                "*.example".to_owned(),
                ".gitignore".to_owned(),
                "config/credentials/".to_owned(),
                "config/tls/".to_owned(),
            ],
        }
    }
}

/// Database size monitoring settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct DbMonitoringSettings {
    /// Whether database size monitoring runs.
    pub enabled: bool,
    /// Emit a warning when any database exceeds this size in MB.
    pub warn_threshold_mb: u64,
    /// Emit an alert when any database exceeds this size in MB.
    pub alert_threshold_mb: u64,
}

impl Default for DbMonitoringSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            warn_threshold_mb: 100,
            alert_threshold_mb: 500,
        }
    }
}

/// Proactive disk space monitoring settings.
///
/// A background task periodically checks available disk space and updates a
/// shared atomic counter. Write paths read the counter to decide whether to
/// proceed, warn, or reject non-essential writes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct DiskSpaceSettings {
    /// Whether disk space monitoring is active.
    pub enabled: bool,
    /// Emit a warning when available space drops below this value (MB).
    pub warning_threshold_mb: u64,
    /// Reject non-essential writes when available space drops below this value (MB).
    pub critical_threshold_mb: u64,
    /// Seconds between background disk space checks.
    pub check_interval_secs: u64,
}

impl Default for DiskSpaceSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            warning_threshold_mb: 1024,
            critical_threshold_mb: 100,
            check_interval_secs: 60,
        }
    }
}

/// `SQLite` corruption recovery settings.
///
/// Controls how the system responds when database corruption is detected:
/// integrity checks on open, automatic backup of corrupt files, and
/// recovery into a fresh database.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "config struct: each bool is an independent toggle"
)]
pub struct SqliteRecoverySettings {
    /// Whether corruption recovery is active.
    pub enabled: bool,
    /// Run `PRAGMA integrity_check` when opening a database.
    pub integrity_check_on_open: bool,
    /// Attempt to dump readable data into a new database on corruption.
    pub auto_repair: bool,
    /// Copy the corrupt file to `{path}.corrupt.{timestamp}` before repair.
    pub backup_corrupt: bool,
}

impl Default for SqliteRecoverySettings {
    fn default() -> Self {
        Self {
            enabled: true,
            integrity_check_on_open: true,
            auto_repair: true,
            backup_corrupt: true,
        }
    }
}

/// Data retention execution settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[derive(Default)]
pub struct RetentionSettings {
    /// Whether automatic retention enforcement (session cleanup) runs.
    pub enabled: bool,
}

/// Sandbox configuration for tool command execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct SandboxSettings {
    /// Whether sandbox restrictions are applied to tool execution.
    pub enabled: bool,
    /// Enforcement level: `enforcing` blocks violations, `permissive` logs them.
    pub enforcement: SandboxEnforcementMode,
    /// Additional filesystem paths granted read access.
    pub extra_read_paths: Vec<PathBuf>,
    /// Additional filesystem paths granted read+write access.
    pub extra_write_paths: Vec<PathBuf>,
    /// Additional filesystem paths granted execute access.
    ///
    /// Values may begin with `~` which is expanded to the HOME environment
    /// variable at policy-build time.
    pub extra_exec_paths: Vec<PathBuf>,
    /// Network egress policy for child processes spawned by the exec tool.
    pub egress: EgressPolicy,
    /// CIDR ranges or addresses permitted when `egress = "allowlist"`.
    pub egress_allowlist: Vec<String>,
}

impl Default for SandboxSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            enforcement: SandboxEnforcementMode::Permissive,
            extra_read_paths: Vec::new(),
            extra_write_paths: Vec::new(),
            extra_exec_paths: Vec::new(),
            egress: EgressPolicy::default(),
            egress_allowlist: Vec::new(),
        }
    }
}

/// Credential resolution configuration.
///
/// Controls how the server discovers LLM API credentials. The `source` field
/// selects the strategy:
///
/// - `"auto"` (default): instance credential file → env vars → Claude Code credentials
/// - `"api-key"`: only instance credential file and env vars
/// - `"claude-code"`: prefer Claude Code's `~/.claude/.credentials.json`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct CredentialConfig {
    /// Credential source strategy: `"auto"`, `"api-key"`, or `"claude-code"`.
    pub source: String,
    /// Override path to the Claude Code credentials file.
    /// Defaults to `~/.claude/.credentials.json`.
    pub claude_code_credentials: Option<String>,
    /// Circuit breaker settings for OAuth token refresh.
    pub circuit_breaker: CircuitBreakerSettings,
}

impl Default for CredentialConfig {
    fn default() -> Self {
        Self {
            source: "auto".to_owned(),
            claude_code_credentials: None,
            circuit_breaker: CircuitBreakerSettings::default(),
        }
    }
}

/// Circuit breaker settings for OAuth token refresh.
///
/// Controls the three-state circuit breaker (Closed → Open → `HalfOpen`)
/// that protects the OAuth refresh endpoint from repeated failed requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct CircuitBreakerSettings {
    /// Number of failures within the window to trip the circuit.
    pub failure_threshold: u32,
    /// Sliding window (seconds) for failure counting.
    pub failure_window_secs: u64,
    /// Base cooldown (seconds) before probing recovery.
    pub cooldown_secs: u64,
    /// Maximum cooldown (seconds) after exponential backoff.
    pub max_cooldown_secs: u64,
}

impl Default for CircuitBreakerSettings {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            failure_window_secs: 60,
            cooldown_secs: 30,
            max_cooldown_secs: 300,
        }
    }
}

/// Structured file logging configuration.
///
/// Controls where server logs are written, how long they are retained, and
/// which minimum severity level is written to the log files.
///
/// Log files are written in JSON format with daily rotation using
/// `tracing_appender`. Old files are pruned after `retention_days` days by
/// the log retention background task.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct LoggingSettings {
    /// Directory where daily log files are written.
    ///
    /// Relative paths are resolved from the instance root. `None` (the
    /// default) resolves to `{instance}/logs/`.
    pub log_dir: Option<String>,
    /// Number of days to retain log files before they are deleted.
    ///
    /// Cleanup is performed once daily at server startup and every 24 hours
    /// thereafter. Default: 14 days.
    pub retention_days: u32,
    /// Minimum log level written to log files.
    ///
    /// Accepts any `tracing` filter directive (e.g. `"warn"`, `"error"`,
    /// `"aletheia=debug,warn"`). Default: `"warn"`, which captures WARN and
    /// ERROR events from all crates regardless of the console log level.
    pub level: String,
    /// Redaction settings for tracing spans and events.
    pub redaction: RedactionSettings,
}

impl Default for LoggingSettings {
    fn default() -> Self {
        Self {
            log_dir: None,
            retention_days: 14,
            level: "warn".to_owned(),
            redaction: RedactionSettings::default(),
        }
    }
}

/// Controls redaction of sensitive data in tracing output.
///
/// When enabled, field values matching sensitive names are replaced with
/// `[REDACTED]`, API key patterns are scrubbed, and long content fields
/// are truncated before reaching any subscriber.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct RedactionSettings {
    /// Master switch for the redaction layer. Default: `true`.
    pub enabled: bool,
    /// Field names whose values are replaced with `[REDACTED]`.
    pub redact_fields: Vec<String>,
    /// Field names whose values are truncated to `truncate_length` chars.
    pub truncate_fields: Vec<String>,
    /// Maximum character length for truncated fields. Default: 200.
    pub truncate_length: usize,
}

impl Default for RedactionSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            redact_fields: vec![
                "token".to_owned(),
                "api_key".to_owned(),
                "secret".to_owned(),
                "password".to_owned(),
                "bearer".to_owned(),
                "authorization".to_owned(),
                "credential".to_owned(),
            ],
            truncate_fields: vec![
                "message".to_owned(),
                "content".to_owned(),
                "body".to_owned(),
                "input".to_owned(),
                "output".to_owned(),
            ],
            truncate_length: 200,
        }
    }
}

/// MCP server configuration.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct McpConfig {
    /// Per-session rate limiting for MCP tool calls.
    pub rate_limit: McpRateLimitConfig,
}

/// Per-session rate limiting configuration for MCP requests.
///
/// Applies separate token bucket limits for expensive operations
/// (`session_message`, `session_create`, `knowledge_search`) and cheap
/// read/status operations. Limits are enforced per MCP session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct McpRateLimitConfig {
    /// Whether MCP rate limiting is active.
    pub enabled: bool,
    /// Maximum requests per minute for expensive operations.
    pub message_requests_per_minute: u32,
    /// Maximum requests per minute for read/status operations.
    pub read_requests_per_minute: u32,
}

impl Default for McpRateLimitConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            message_requests_per_minute: 60,
            read_requests_per_minute: 300,
        }
    }
}
