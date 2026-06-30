//! Prosoche attention and self-audit scheduling configuration.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

const DEFAULT_EXTERNAL_TIMER_TASK_ID: &str = "prosoche-self-audit";

/// Task identifier advertised to an external prosoche timer.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProsocheExternalTimerTaskId(String);

impl ProsocheExternalTimerTaskId {
    /// Create a prosoche external timer task identifier from a string-like value.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Return the task identifier as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for ProsocheExternalTimerTaskId {
    fn default() -> Self {
        Self(DEFAULT_EXTERNAL_TIMER_TASK_ID.to_owned())
    }
}

impl fmt::Debug for ProsocheExternalTimerTaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ProsocheExternalTimerTaskId")
            .field(&self.0)
            .finish()
    }
}

impl fmt::Display for ProsocheExternalTimerTaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for ProsocheExternalTimerTaskId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<&str> for ProsocheExternalTimerTaskId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl AsRef<str> for ProsocheExternalTimerTaskId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl PartialEq<&str> for ProsocheExternalTimerTaskId {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

/// Prosoche attention and self-audit scheduling configuration.
// kanon:ignore RUST/no-debug-derive-on-public-types -- WHY: prosoche schedule config contains scheduler knobs and task identifiers only, no secrets.
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
    task_id: Option<ProsocheExternalTimerTaskId>,
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
// kanon:ignore RUST/no-debug-derive-on-public-types -- WHY: enum variants identify scheduling mode only, no secrets.
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
// kanon:ignore RUST/no-debug-derive-on-public-types -- WHY: prosoche task schedule settings contain booleans, intervals, and local active windows only.
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
// kanon:ignore RUST/no-debug-derive-on-public-types -- WHY: active window settings contain only local hour bounds.
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
// kanon:ignore RUST/no-debug-derive-on-public-types -- WHY: external timer settings contain scheduling metadata only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct ProsocheExternalTimerSettings {
    /// Whether the external timer trigger is enabled.
    pub enabled: bool,
    /// Task identifier advertised to the external timer.
    pub task_id: ProsocheExternalTimerTaskId,
    /// Expected interval, in seconds, between external timer triggers.
    pub interval_secs: u64,
}

impl Default for ProsocheExternalTimerSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            task_id: ProsocheExternalTimerTaskId::default(),
            interval_secs: 300,
        }
    }
}
