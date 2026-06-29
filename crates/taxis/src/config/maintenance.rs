//! Maintenance, monitoring, sandbox, logging, and MCP configuration types.

use std::path::PathBuf;

use serde::{Deserialize, Deserializer, Serialize};

use super::{EgressPolicy, SandboxEnforcementMode};

/// Instance maintenance settings.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct MaintenanceSettings {
    /// Trace log file rotation and compression.
    pub trace_rotation: TraceRotationSettings,
    /// Filesystem drift detection against expected instance layout.
    pub drift_detection: DriftDetectionSettings,
    /// Database size monitoring and alerting.
    pub db_monitoring: DbMonitoringSettings,
    /// Proactive disk space monitoring and write protection.
    pub disk_space: DiskSpaceSettings,
    /// Automatic data retention enforcement.
    pub retention: RetentionSettings,
    /// Whether background knowledge graph maintenance tasks are enabled.
    #[serde(default)]
    pub knowledge_maintenance_enabled: bool,
    /// Serendipity discovery maintenance settings.
    pub knowledge_maintenance_serendipity: SerendipityMaintenanceSettings,
    /// Watchdog process monitor settings.
    pub watchdog: WatchdogSettings,
    /// Periodic cron task settings (evolution, reflection, graph cleanup).
    pub cron_tasks: CronTaskSettings,
    /// Whole-instance backup set settings.
    pub backup: BackupSettings,
    /// Prosoche attention and self-audit scheduling settings.
    pub prosoche: ProsocheMaintenanceSettings,
}

/// Prosoche attention and self-audit scheduling configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProsocheMaintenanceSettings {
    /// How prosoche tasks are scheduled.
    pub mode: ProsocheScheduleMode,
    /// Periodic prosoche attention check ("heartbeat") schedule.
    pub heartbeat: ProsocheTaskScheduleSettings,
    /// Periodic prosoche self-audit schedule.
    pub self_audit: ProsocheTaskScheduleSettings,
    /// External timer integration for prosoche self-audit.
    pub external_timer: ProsocheExternalTimerSettings,
}

impl Default for ProsocheMaintenanceSettings {
    fn default() -> Self {
        Self {
            mode: ProsocheScheduleMode::default(),
            heartbeat: ProsocheTaskScheduleSettings {
                enabled: true,
                interval_secs: 45 * 60,
                active_window: Some(ProsocheActiveWindowSettings::default()),
            },
            self_audit: ProsocheTaskScheduleSettings {
                enabled: true,
                interval_secs: 6 * 3600,
                active_window: Some(ProsocheActiveWindowSettings::default()),
            },
            external_timer: ProsocheExternalTimerSettings::default(),
        }
    }
}

impl<'de> Deserialize<'de> for ProsocheMaintenanceSettings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let overrides = ProsocheMaintenanceOverrides::deserialize(deserializer)?;
        let mut settings = Self::default();

        if let Some(mode) = overrides.mode {
            settings.mode = mode;
        }
        if let Some(heartbeat) = overrides.heartbeat {
            heartbeat.apply_to(&mut settings.heartbeat);
        }
        if let Some(self_audit) = overrides.self_audit {
            self_audit.apply_to(&mut settings.self_audit);
        }
        if let Some(external_timer) = overrides.external_timer {
            external_timer.apply_to(&mut settings.external_timer);
        }

        Ok(settings)
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
struct ProsocheMaintenanceOverrides {
    mode: Option<ProsocheScheduleMode>,
    heartbeat: Option<ProsocheTaskScheduleOverrides>,
    self_audit: Option<ProsocheTaskScheduleOverrides>,
    external_timer: Option<ProsocheExternalTimerOverrides>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
struct ProsocheTaskScheduleOverrides {
    enabled: Option<bool>,
    interval_secs: Option<u64>,
    #[expect(
        clippy::option_option,
        reason = "override parsing must distinguish omitted activeWindow from explicit null"
    )]
    active_window: Option<Option<ProsocheActiveWindowSettings>>,
}

impl ProsocheTaskScheduleOverrides {
    fn apply_to(self, settings: &mut ProsocheTaskScheduleSettings) {
        if let Some(enabled) = self.enabled {
            settings.enabled = enabled;
        }
        if let Some(interval_secs) = self.interval_secs {
            settings.interval_secs = interval_secs;
        }
        if let Some(active_window) = self.active_window {
            settings.active_window = active_window;
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
struct ProsocheExternalTimerOverrides {
    enabled: Option<bool>,
    task_id: Option<String>,
    interval_secs: Option<u64>,
}

impl ProsocheExternalTimerOverrides {
    fn apply_to(self, settings: &mut ProsocheExternalTimerSettings) {
        if let Some(enabled) = self.enabled {
            settings.enabled = enabled;
        }
        if let Some(task_id) = self.task_id {
            settings.task_id = task_id;
        }
        if let Some(interval_secs) = self.interval_secs {
            settings.interval_secs = interval_secs;
        }
    }
}

/// How prosoche background tasks are driven.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum ProsocheScheduleMode {
    /// Schedule prosoche tasks through the daemon's internal scheduler.
    #[default]
    Daemon,
    /// Trigger prosoche self-audit through an external timer only.
    External,
    /// Use both the daemon scheduler and an external timer.
    Both,
    /// Disable all prosoche scheduling.
    Disabled,
}

impl ProsocheScheduleMode {
    /// Whether the internal daemon scheduler should run prosoche tasks.
    #[must_use]
    pub const fn runs_daemon_tasks(&self) -> bool {
        matches!(self, Self::Daemon | Self::Both)
    }

    /// Whether an external timer may trigger prosoche self-audit.
    #[must_use]
    pub const fn uses_external_timer(&self) -> bool {
        matches!(self, Self::External | Self::Both)
    }
}

/// Schedule settings for a single prosoche task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct ProsocheTaskScheduleSettings {
    /// Whether this prosoche task is enabled.
    pub enabled: bool,
    /// Seconds between runs.
    pub interval_secs: u64,
    /// Optional local-time active window. When `None`, the task may run at any hour.
    pub active_window: Option<ProsocheActiveWindowSettings>,
}

impl Default for ProsocheTaskScheduleSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_secs: 3600,
            active_window: None,
        }
    }
}

/// Local-time active window for a prosoche task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct ProsocheActiveWindowSettings {
    /// First hour (inclusive) when the task may run. Range 0..=23.
    pub start_hour: u8,
    /// Last hour (exclusive) when the task may run. Range 0..=24.
    pub end_hour: u8,
}

impl Default for ProsocheActiveWindowSettings {
    fn default() -> Self {
        Self {
            start_hour: 8,
            end_hour: 23,
        }
    }
}

/// External timer integration for prosoche self-audit.
///
/// Used when `ProsocheScheduleMode::External` or `Both` is selected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct ProsocheExternalTimerSettings {
    /// Whether the external timer trigger is enabled.
    pub enabled: bool,
    /// Task identifier advertised to the external timer.
    pub task_id: String,
    /// Expected interval, in seconds, between external timer triggers.
    pub interval_secs: u64,
}

impl Default for ProsocheExternalTimerSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            task_id: "prosoche-self-audit".to_owned(),
            interval_secs: 300,
        }
    }
}

/// Serendipity discovery maintenance settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct SerendipityMaintenanceSettings {
    /// Whether the serendipity discovery task is enabled.
    pub enabled: bool,
    /// Cron cadence used when the task is scheduled.
    pub cadence: String,
}

impl Default for SerendipityMaintenanceSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            cadence: "0 0 7 * * *".to_owned(),
        }
    }
}

/// Trace file rotation settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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

/// Data retention execution settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct RetentionSettings {
    /// Whether automatic retention enforcement (session cleanup) runs.
    pub enabled: bool,
    /// Number of days after `updated_at` before a closed session is eligible
    /// for deletion. `None` means no session cleanup runs regardless of
    /// `enabled`.
    #[serde(alias = "sessionMaxAgeDays", alias = "session_max_age_days")]
    pub closed_session_ttl_days: Option<u32>,
    /// Number of days after which orphaned messages are eligible for cleanup.
    #[serde(
        alias = "orphanMessageMaxAgeDays",
        alias = "orphan_message_max_age_days"
    )]
    pub orphan_message_max_age_days: Option<u32>,
    /// Maximum sessions to retain per agent. `0` means unlimited.
    #[serde(alias = "maxSessionsPerNous", alias = "max_sessions_per_nous")]
    pub max_sessions_per_nous: u32,
    /// When `true` (the default), closed sessions are exported to a JSON
    /// archive before hard deletion. Set to `false` only when immediate
    /// deletion without an archive artifact is explicitly desired.
    #[serde(default = "default_archive_before_delete")]
    pub archive_before_delete: bool,
    /// Number of days to retain session JSON archives under
    /// `<data_dir>/archive/sessions/` before pruning. `None` disables pruning.
    ///
    /// WHY: `archive_before_delete` writes a full JSON dump for every deleted
    /// session. Without a TTL the archive directory grows monotonically and can
    /// exhaust disk (#5658).
    #[serde(
        alias = "archiveTtlDays",
        alias = "archive_ttl_days",
        default = "default_archive_ttl_days"
    )]
    pub archive_ttl_days: Option<u32>,
}

fn default_archive_before_delete() -> bool {
    true
}

// WHY(#5658): serde `default = "..."` requires the function return type to
// match the field type exactly; `Option<u32>` allows `None` to disable archive
// pruning. The function always returns `Some(90)` triggering
// `unnecessary_wraps`, but changing the return type would break the serde
// default wiring without a full custom `Deserialize` impl.
#[expect(
    clippy::unnecessary_wraps,
    reason = "WHY(#5658): serde default fn must return Option<u32> to match the field type"
)]
fn default_archive_ttl_days() -> Option<u32> {
    // WHY: 90 days bounds archive growth by default while keeping a reasonable
    // recovery window for recently deleted sessions (#5658).
    Some(90)
}

impl Default for RetentionSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            closed_session_ttl_days: None,
            orphan_message_max_age_days: None,
            max_sessions_per_nous: 0,
            archive_before_delete: true,
            archive_ttl_days: default_archive_ttl_days(),
        }
    }
}

/// Sandbox configuration for tool command execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct SandboxSettings {
    /// Whether sandbox restrictions are applied to tool execution.
    pub enabled: bool,
    /// Enforcement level: `enforcing` blocks violations, `permissive` logs them.
    pub enforcement: SandboxEnforcementMode,
    /// Default filesystem root granted read access.
    ///
    /// Defaults to `~` (HOME). Operators can set this to a stricter path.
    /// The `~` prefix is expanded to the HOME environment variable at runtime.
    ///
    /// WHY: without a home-directory default, agents cannot read user files.
    pub allowed_root: PathBuf,
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
    /// Maximum number of processes (`RLIMIT_NPROC`) for exec child processes.
    ///
    /// WHY: `RLIMIT_NPROC` counts ALL processes for the user, not just sandbox
    /// children. Default: 256.
    pub nproc_limit: u32,
}

impl Default for SandboxSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            enforcement: SandboxEnforcementMode::Permissive,
            allowed_root: PathBuf::from("~"),
            extra_read_paths: Vec::new(),
            extra_write_paths: Vec::new(),
            extra_exec_paths: Vec::new(),
            egress: EgressPolicy::default(),
            egress_allowlist: Vec::new(),
            nproc_limit: 256,
        }
    }
}

/// Default value used for `CredentialConfig::refresh_threshold_secs`.
pub(crate) const DEFAULT_REFRESH_THRESHOLD_SECS: u64 = 3_600;

/// Credential resolution configuration.
///
/// Controls how the server discovers LLM API credentials. The `source` field
/// selects the strategy:
///
/// - `"auto"` (default): instance credential file → keyring → env vars
/// - `"api-key"`: only instance credential file and env vars
/// - `"claude-code"`: prefer an explicit Claude Code credentials path
// kanon:ignore RUST/no-debug-derive-on-public-types — CredentialConfig holds only env-var names and strategy strings, not actual secrets; derived Debug leaks no credentials
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct CredentialConfig {
    /// Credential source strategy: `"auto"`, `"api-key"`, or `"claude-code"`.
    pub source: String,
    /// Explicit path to the Claude Code credentials file.
    /// Also configurable with `CLAUDE_CODE_CREDS`.
    pub claude_code_credentials: Option<String>,
    /// Refresh when token has less than this many seconds remaining.
    pub refresh_threshold_secs: u64,
    /// Circuit breaker settings for OAuth token refresh.
    pub circuit_breaker: CircuitBreakerSettings,
}

impl Default for CredentialConfig {
    fn default() -> Self {
        Self {
            source: "auto".to_owned(),
            claude_code_credentials: None,
            refresh_threshold_secs: DEFAULT_REFRESH_THRESHOLD_SECS,
            circuit_breaker: CircuitBreakerSettings::default(),
        }
    }
}

#[cfg(test)]
const _: () =
    assert!(DEFAULT_REFRESH_THRESHOLD_SECS == symbolon::credential::REFRESH_THRESHOLD_SECS);

/// Circuit breaker settings for OAuth token refresh.
///
/// Controls the three-state circuit breaker (Closed → Open → `HalfOpen`)
/// that protects the OAuth refresh endpoint from repeated failed requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct RedactionSettings {
    /// Primary switch for the redaction layer. Default: `true`.
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

/// Watchdog process monitor settings.
///
/// Monitors agent processes for health via heartbeat and auto-restarts
/// hung or failed components with exponential backoff.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct WatchdogSettings {
    /// Whether the watchdog monitor is enabled.
    pub enabled: bool,
    /// Seconds without a heartbeat before a process is declared hung.
    pub heartbeat_timeout_secs: u64,
    /// Seconds between watchdog health check sweeps.
    pub check_interval_secs: u64,
    /// Maximum restart attempts before abandoning a process.
    pub max_restarts: u32,
}

impl Default for WatchdogSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            heartbeat_timeout_secs: 60,
            check_interval_secs: 10,
            max_restarts: 5,
        }
    }
}

/// Whole-instance backup set periodic backup settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct BackupSettings {
    /// Whether automatic whole-instance backup sets are enabled.
    pub enabled: bool,
    /// Hours between automatic backups.
    pub backup_interval_hours: u64,
    /// Maximum number of backup snapshots to retain.
    pub backup_retention_count: usize,
}

impl Default for BackupSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            backup_interval_hours: 24,
            backup_retention_count: 7,
        }
    }
}

/// Periodic cron task settings.
///
/// All cron tasks are disabled by default. Each task runs on a configurable
/// interval and is dispatched via the daemon bridge or knowledge executor.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct CronTaskSettings {
    /// Evolution: periodic configuration variant search.
    pub evolution: CronTaskEntry,
    /// Reflection: periodic self-reflection prompt.
    pub reflection: CronTaskEntry,
    /// Graph cleanup: periodic knowledge graph orphan removal.
    pub graph_cleanup: CronTaskEntry,
}

impl Default for CronTaskSettings {
    fn default() -> Self {
        Self {
            evolution: CronTaskEntry {
                enabled: false,
                interval_secs: 24 * 3600,
            },
            reflection: CronTaskEntry {
                enabled: false,
                interval_secs: 24 * 3600,
            },
            graph_cleanup: CronTaskEntry {
                enabled: false,
                interval_secs: 7 * 24 * 3600,
            },
        }
    }
}

/// Configuration for a single cron task.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct CronTaskEntry {
    /// Whether this cron task is enabled.
    pub enabled: bool,
    /// Interval between runs in seconds.
    pub interval_secs: u64,
}

impl Default for CronTaskEntry {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_secs: 24 * 3600,
        }
    }
}

/// Prompt audit log configuration (#3411).
///
/// Controls the operator-visible append-only JSONL log of every outbound
/// LLM `CompletionRequest`. See `nous::audit` for the record schema and
/// sovereignty contract on what is (and is not) logged.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct PromptAuditSettings {
    /// Whether outbound requests are recorded. Default: `true`.
    ///
    /// WHY default-on: this is a sovereignty feature — operators need
    /// visibility into what the system sends to external providers without
    /// opting in. The log stores hashes and IDs, not content, so the cost
    /// of enabling it is small.
    pub enabled: bool,
    /// Directory for daily JSONL files. When `None`, resolves to
    /// `{instance}/logs/prompt-audit/` at startup.
    pub log_dir: Option<PathBuf>,
    /// Days to retain JSONL files before the daemon prunes them.
    pub retention_days: u32,
    /// Whether the IDs and sensitivity labels of facts filtered by the
    /// sensitivity policy (#3404) are included in each record. Default: `true`.
    ///
    /// Set to `false` to retain included fact IDs while writing an empty
    /// `fact_ids_filtered` list, avoiding persistence of identifiers for facts
    /// withheld from the prompt.
    pub include_filtered_ids: bool,
}

impl Default for PromptAuditSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            log_dir: None,
            retention_days: 90,
            include_filtered_ids: true,
        }
    }
}

/// MCP server configuration.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct McpConfig {
    /// Per-session rate limiting for MCP tool calls.
    pub rate_limit: McpRateLimitConfig,
    /// Knowledge graph MCP surface configuration.
    pub knowledge_graph: KnowledgeGraphMcpConfig,
    /// Repomix MCP surface configuration.
    pub repomix: RepomixMcpConfig,
}

/// Configuration for the knowledge graph MCP surface.
///
/// When enabled, the MCP server exposes `knowledge.*` tools for querying
/// and mutating the knowledge graph. Read operations require `Agent` role;
/// mutations (`insert`, `forget`) require `Operator` role.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeGraphMcpConfig {
    /// Whether the knowledge graph MCP tools are enabled.
    ///
    /// Defaults to `false` — operators must explicitly opt in.
    pub enabled: bool,
    /// Maximum number of results returned by `knowledge.search`.
    ///
    /// Defaults to 50.
    pub max_search_results: u32,
    /// Maximum graph traversal depth for `knowledge.graph_neighbors`.
    ///
    /// Capped at 4 to prevent unbounded Datalog recursion.
    pub max_graph_depth: u32,
}

impl Default for KnowledgeGraphMcpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_search_results: 50,
            max_graph_depth: 2,
        }
    }
}

/// Configuration for the repomix MCP surface.
///
/// When enabled, the MCP server exposes `repomix.*` tools for packing
/// crate source code into token-efficient context windows. Read operations
/// (`templates_list`, `template_get`) require `Agent` role; the pack
/// operation requires `Operator` role because it can be expensive and
/// generates dispatch context.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct RepomixMcpConfig {
    /// Whether the repomix MCP tools are enabled.
    ///
    /// Defaults to `false` — operators must explicitly opt in.
    pub enabled: bool,
    /// Maximum output tokens for a packed context.
    ///
    /// Defaults to `128_000` (Claude 3.5 Sonnet context window).
    pub max_output_tokens: u32,
    /// Directory containing custom `.repomix` template files.
    ///
    /// When `None`, built-in templates (`single_crate`, `crate_with_deps`,
    /// `cross_crate`) are used.
    pub templates_dir: Option<String>,
}

impl Default for RepomixMcpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_output_tokens: 128_000,
            templates_dir: None,
        }
    }
}

/// Per-session rate limiting configuration for MCP requests.
///
/// Applies separate token bucket limits for expensive operations
/// (`session_message`, `session_create`, `knowledge_search`) and cheap
/// read/status operations. Limits are enforced per MCP session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
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
