//! Structured pack health reporting.
//!
//! Pack loading and tool registration degrade gracefully: invalid packs are
//! skipped, missing optional context files are dropped, and unregistrable
//! tools are refused. Without a structured record, operators and
//! control-plane views cannot distinguish a fully active pack from a
//! partially degraded one. The types here are that record: one
//! [`PackHealth`] per configured pack, aggregated into a [`PackReport`].

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::tools::PackToolFailure;

/// Overall activation state of a single domain pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum PackStatus {
    /// Pack loaded cleanly; every declared component is active.
    Active,
    /// Pack is active, but at least one declared component (context entry,
    /// tool, overlay power) was skipped, dropped, or failed.
    Degraded,
    /// Pack is not active at all: its manifest or a required context entry
    /// failed to load.
    Failed,
}

/// Which part of a pack an issue belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum PackComponent {
    /// `pack.toml` parsing or validation.
    Manifest,
    /// A `[[context]]` entry.
    Context,
    /// A `[[tools]]` entry.
    Tool,
    /// An `[overlays.*]` entry.
    Overlay,
}

/// Severity of a single pack issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Severity {
    /// Operator-visible note about an applied behavior (e.g. an overlay
    /// power that is in effect).
    Info,
    /// A component was skipped or dropped; the pack stays active.
    Warning,
    /// A component failed; the pack is degraded or not active.
    Error,
}

/// A single structured issue recorded against a pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackIssue {
    /// Which part of the pack the issue belongs to.
    pub component: PackComponent,
    /// Issue severity.
    pub severity: Severity,
    /// Human-readable description, suitable for a startup log line.
    pub message: String,
}

/// Health of one configured pack after load and tool registration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackHealth {
    /// Pack name from the manifest, or the path-derived fallback when the
    /// manifest itself could not be read.
    pub name: String,
    /// Configured pack root path.
    pub path: PathBuf,
    /// Overall activation state.
    pub status: PackStatus,
    /// Issues recorded against this pack, in the order they occurred.
    pub issues: Vec<PackIssue>,
}

impl PackHealth {
    /// A cleanly loaded pack with no issues yet.
    #[must_use]
    pub(crate) fn active(name: String, path: PathBuf) -> Self {
        Self {
            name,
            path,
            status: PackStatus::Active,
            issues: Vec::new(),
        }
    }

    /// A pack that failed to load at all.
    #[must_use]
    pub(crate) fn failed(path: PathBuf, error: &crate::error::Error) -> Self {
        let name = path
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("unknown")
            .to_owned();
        Self {
            name,
            path,
            status: PackStatus::Failed,
            issues: vec![PackIssue {
                component: PackComponent::Manifest,
                severity: Severity::Error,
                message: error.to_string(),
            }],
        }
    }

    /// Record an issue, degrading an active pack. `Failed` is terminal and
    /// never upgrades.
    pub(crate) fn push_issue(&mut self, issue: PackIssue) {
        if self.status == PackStatus::Active && issue.severity != Severity::Info {
            self.status = PackStatus::Degraded;
        }
        self.issues.push(issue);
    }
}

/// Aggregated health of every configured pack.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PackReport {
    /// Per-pack health, in config order.
    pub packs: Vec<PackHealth>,
    /// Report-level notes about the host's pack execution support, e.g.
    /// reduced subprocess enforcement off-Linux (#5215). Empty on a fully
    /// supported platform.
    pub notes: Vec<String>,
}

/// Count of packs by status, for summary logging.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StatusCounts {
    /// Packs with [`PackStatus::Active`].
    pub active: usize,
    /// Packs with [`PackStatus::Degraded`].
    pub degraded: usize,
    /// Packs with [`PackStatus::Failed`].
    pub failed: usize,
}

impl PackReport {
    /// Count packs by status.
    #[must_use]
    pub fn status_counts(&self) -> StatusCounts {
        let mut counts = StatusCounts::default();
        for pack in &self.packs {
            match pack.status {
                PackStatus::Active => counts.active += 1,
                PackStatus::Degraded => counts.degraded += 1,
                PackStatus::Failed => counts.failed += 1,
            }
        }
        counts
    }

    /// `true` when at least one pack failed to activate.
    #[must_use]
    pub fn has_failures(&self) -> bool {
        self.packs.iter().any(|p| p.status == PackStatus::Failed)
    }

    /// Fold tool registration failures into the per-pack records.
    ///
    /// A pack whose tool fails to register stays active but degrades.
    pub fn record_tool_failures(&mut self, failures: &[PackToolFailure]) {
        for failure in failures {
            let issue = PackIssue {
                component: PackComponent::Tool,
                severity: Severity::Error,
                message: format!(
                    "tool '{}' failed to register: {}",
                    failure.tool_name, failure.error
                ),
            };
            if let Some(health) = self.packs.iter_mut().find(|p| p.name == failure.pack_name) {
                health.push_issue(issue);
            } else {
                // WHY: registration only sees loaded packs, so a missing
                // entry means a loader/registration mismatch. Keep the
                // failure visible rather than dropping it silently.
                let mut health = PackHealth::active(failure.pack_name.clone(), PathBuf::new());
                health.push_issue(issue);
                self.packs.push(health);
            }
        }
    }
}

/// Host capability notes for pack tool execution (#5215).
///
/// Empty on Linux, where the full subprocess contract (process-group kill,
/// `RLIMIT_NPROC`/`RLIMIT_CPU` resource limits) is enforced. On other
/// platforms the degraded guarantees are named so operators see reduced
/// enforcement instead of assuming it.
#[must_use]
pub fn platform_notes() -> Vec<String> {
    #[cfg(target_os = "linux")]
    {
        Vec::new()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let mut notes = vec![
            "subprocess resource limits (RLIMIT_NPROC, RLIMIT_CPU) are not enforced on this \
             platform; pack tool wall-clock timeouts still apply"
                .to_owned(),
        ];
        #[cfg(not(unix))]
        notes.push(
            "pack shell tools declare platforms = [\"unix\"] by default and are skipped on \
             this platform unless a pack opts into \"windows\""
                .to_owned(),
        );
        notes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_pack_is_active() {
        let health = PackHealth::active("good".to_owned(), PathBuf::from("/packs/good"));
        assert_eq!(health.status, PackStatus::Active);
        assert!(health.issues.is_empty());
    }

    #[test]
    fn warning_issue_degrades_active_pack() {
        let mut health = PackHealth::active("partial".to_owned(), PathBuf::from("/packs/partial"));
        health.push_issue(PackIssue {
            component: PackComponent::Context,
            severity: Severity::Warning,
            message: "context file 'missing.md' skipped".to_owned(),
        });
        assert_eq!(health.status, PackStatus::Degraded);
        assert_eq!(health.issues.len(), 1);
    }

    #[test]
    fn info_issue_does_not_degrade() {
        let mut health = PackHealth::active("noted".to_owned(), PathBuf::from("/packs/noted"));
        health.push_issue(PackIssue {
            component: PackComponent::Overlay,
            severity: Severity::Info,
            message: "model override in effect".to_owned(),
        });
        assert_eq!(health.status, PackStatus::Active);
    }

    #[test]
    fn failed_pack_stays_failed() {
        let err = crate::error::Error::ManifestNotFound {
            path: PathBuf::from("/packs/gone/pack.toml"),
            location: snafu::location!(),
        };
        let mut health = PackHealth::failed(PathBuf::from("/packs/gone"), &err);
        assert_eq!(health.status, PackStatus::Failed);
        health.push_issue(PackIssue {
            component: PackComponent::Tool,
            severity: Severity::Info,
            message: "irrelevant".to_owned(),
        });
        assert_eq!(health.status, PackStatus::Failed);
        assert_eq!(health.name, "gone");
    }

    #[test]
    fn report_counts_by_status() {
        let mut report = PackReport::default();
        report
            .packs
            .push(PackHealth::active("a".to_owned(), PathBuf::new()));
        let mut degraded = PackHealth::active("b".to_owned(), PathBuf::new());
        degraded.push_issue(PackIssue {
            component: PackComponent::Tool,
            severity: Severity::Error,
            message: "bad tool".to_owned(),
        });
        report.packs.push(degraded);
        let err = crate::error::Error::ParseManifest {
            path: PathBuf::from("/packs/c/pack.toml"),
            reason: "bad toml".to_owned(),
            location: snafu::location!(),
        };
        report
            .packs
            .push(PackHealth::failed(PathBuf::from("/packs/c"), &err));

        let counts = report.status_counts();
        assert_eq!(counts.active, 1);
        assert_eq!(counts.degraded, 1);
        assert_eq!(counts.failed, 1);
        assert!(report.has_failures());
    }
}
