//! Prosoche maintenance status formatting.

use serde::Serialize;

use oikonomos::schedule::TaskStatus;

/// Active path for prosoche heartbeat/self-audit maintenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ProsochePath {
    DaemonScheduler,
    ExternalTimer,
    Both,
    Disabled,
}

impl ProsochePath {
    fn from_mode(runs_daemon: bool, uses_external: bool) -> Self {
        match (runs_daemon, uses_external) {
            (true, true) => Self::Both,
            (true, false) => Self::DaemonScheduler,
            (false, true) => Self::ExternalTimer,
            (false, false) => Self::Disabled,
        }
    }
}

impl std::fmt::Display for ProsochePath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DaemonScheduler => write!(f, "daemon scheduler"),
            Self::ExternalTimer => write!(f, "external timer"),
            Self::Both => write!(f, "both"),
            Self::Disabled => write!(f, "disabled"),
        }
    }
}

/// Summary of the active prosoche heartbeat path for status output.
#[derive(Debug, Clone, Serialize)]
pub(super) struct ProsochePathSummary {
    path: ProsochePath,
    heartbeat_enabled: bool,
    self_audit_enabled: bool,
    external_timer_enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    heartbeat_interval_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    self_audit_interval_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    external_timer_interval_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    external_timer_task_id: Option<String>,
}

pub(super) fn prosoche_path_summary(
    settings: &taxis::config::ProsocheMaintenanceSettings,
) -> ProsochePathSummary {
    let daemon_mode = settings.mode.runs_daemon_tasks();
    let heartbeat_active = daemon_mode && settings.heartbeat.enabled;
    let self_audit_active = daemon_mode && settings.self_audit.enabled;
    let external_active = settings.mode.uses_external_timer() && settings.external_timer.enabled;
    let path = ProsochePath::from_mode(heartbeat_active || self_audit_active, external_active);
    ProsochePathSummary {
        path,
        heartbeat_enabled: heartbeat_active,
        self_audit_enabled: self_audit_active,
        external_timer_enabled: external_active,
        heartbeat_interval_secs: heartbeat_active.then_some(settings.heartbeat.interval_secs),
        self_audit_interval_secs: self_audit_active.then_some(settings.self_audit.interval_secs),
        external_timer_interval_secs: external_active
            .then_some(settings.external_timer.interval_secs),
        external_timer_task_id: external_active
            .then(|| settings.external_timer.task_id.as_str().to_owned()),
    }
}

pub(super) fn format_prosoche_path(summary: &ProsochePathSummary) -> String {
    match summary.path {
        ProsochePath::DaemonScheduler => format!(
            "Prosoche heartbeat: {} (heartbeat: {}s, self-audit: {}s)",
            summary.path,
            summary.heartbeat_interval_secs.unwrap_or(0),
            summary.self_audit_interval_secs.unwrap_or(0)
        ),
        ProsochePath::ExternalTimer => format!(
            "Prosoche heartbeat: {} (task-id: {}, interval: {}s)",
            summary.path,
            summary.external_timer_task_id.as_deref().unwrap_or("none"),
            summary.external_timer_interval_secs.unwrap_or(0)
        ),
        ProsochePath::Both => format!(
            "Prosoche heartbeat: {} (heartbeat: {}s, self-audit: {}s, external task-id: {}, interval: {}s)",
            summary.path,
            summary.heartbeat_interval_secs.unwrap_or(0),
            summary.self_audit_interval_secs.unwrap_or(0),
            summary.external_timer_task_id.as_deref().unwrap_or("none"),
            summary.external_timer_interval_secs.unwrap_or(0)
        ),
        ProsochePath::Disabled => "Prosoche heartbeat: disabled".to_owned(),
    }
}

#[derive(Debug, Serialize)]
pub(super) struct MaintenanceStatusOutput {
    pub(super) tasks: Vec<TaskStatus>,
    pub(super) prosoche: ProsochePathSummary,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prosoche_path_from_mode_combinations() {
        assert_eq!(
            ProsochePath::from_mode(false, false),
            ProsochePath::Disabled
        );
        assert_eq!(
            ProsochePath::from_mode(true, false),
            ProsochePath::DaemonScheduler
        );
        assert_eq!(
            ProsochePath::from_mode(false, true),
            ProsochePath::ExternalTimer
        );
        assert_eq!(ProsochePath::from_mode(true, true), ProsochePath::Both);
    }

    #[test]
    fn prosoche_path_display_labels() {
        assert_eq!(
            ProsochePath::DaemonScheduler.to_string(),
            "daemon scheduler"
        );
        assert_eq!(ProsochePath::ExternalTimer.to_string(), "external timer");
        assert_eq!(ProsochePath::Both.to_string(), "both");
        assert_eq!(ProsochePath::Disabled.to_string(), "disabled");
    }

    #[test]
    fn format_prosoche_path_outputs_active_path() {
        let daemon = ProsochePathSummary {
            path: ProsochePath::DaemonScheduler,
            heartbeat_enabled: true,
            self_audit_enabled: true,
            external_timer_enabled: false,
            heartbeat_interval_secs: Some(60),
            self_audit_interval_secs: Some(300),
            external_timer_interval_secs: None,
            external_timer_task_id: None,
        };
        assert!(format_prosoche_path(&daemon).contains("daemon scheduler"));
        assert!(format_prosoche_path(&daemon).contains("heartbeat: 60s"));
        assert!(format_prosoche_path(&daemon).contains("self-audit: 300s"));

        let external = ProsochePathSummary {
            path: ProsochePath::ExternalTimer,
            heartbeat_enabled: false,
            self_audit_enabled: false,
            external_timer_enabled: true,
            heartbeat_interval_secs: None,
            self_audit_interval_secs: None,
            external_timer_interval_secs: Some(300),
            external_timer_task_id: Some("task-42".to_owned()),
        };
        assert!(format_prosoche_path(&external).contains("external timer"));
        assert!(format_prosoche_path(&external).contains("task-id: task-42"));
        assert!(format_prosoche_path(&external).contains("interval: 300s"));

        let both = ProsochePathSummary {
            path: ProsochePath::Both,
            heartbeat_enabled: true,
            self_audit_enabled: true,
            external_timer_enabled: true,
            heartbeat_interval_secs: Some(60),
            self_audit_interval_secs: Some(300),
            external_timer_interval_secs: Some(300),
            external_timer_task_id: Some("task-42".to_owned()),
        };
        assert!(format_prosoche_path(&both).contains("both"));
        assert!(format_prosoche_path(&both).contains("external task-id: task-42"));

        let disabled = ProsochePathSummary {
            path: ProsochePath::Disabled,
            heartbeat_enabled: false,
            self_audit_enabled: false,
            external_timer_enabled: false,
            heartbeat_interval_secs: None,
            self_audit_interval_secs: None,
            external_timer_interval_secs: None,
            external_timer_task_id: None,
        };
        assert_eq!(
            format_prosoche_path(&disabled),
            "Prosoche heartbeat: disabled"
        );
    }
}
