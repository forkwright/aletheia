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

/// Stable identity of one configured pack occurrence.
///
/// The same path may appear more than once in configuration and different
/// manifests may use the same name, so neither path nor name identifies the
/// occurrence whose later registration failed (#5208).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PackInstanceId(usize);

impl PackInstanceId {
    /// Build an ID from the pack's zero-based position in configured order.
    #[must_use]
    pub const fn from_ordinal(ordinal: usize) -> Self {
        Self(ordinal)
    }

    /// Return the zero-based configured position.
    #[must_use]
    pub const fn ordinal(self) -> usize {
        self.0
    }
}

/// Overall activation state of a single domain pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum PackStatus {
    /// Pack loaded without a reported degradation. Registration and runtime
    /// reconciliation happen later; their failures may subsequently be folded
    /// into this health entry, so this state makes no effectiveness claim.
    Active,
    /// Pack loaded, but at least one declared component (context entry, tool,
    /// overlay power) was skipped, dropped, or failed.
    Degraded,
    /// Pack did not load: its manifest or a required context entry
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
    /// Operator-visible note about an admitted behavior (e.g. an overlay
    /// power retained by policy for later runtime reconciliation).
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

/// Health of one configured pack after load, optionally enriched by later
/// registration and runtime-reconciliation outcomes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackHealth {
    /// Stable configured-occurrence identity.
    #[serde(default)]
    pub instance_id: PackInstanceId,
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
    pub(crate) fn active(instance_id: PackInstanceId, name: String, path: PathBuf) -> Self {
        Self {
            instance_id,
            name,
            path,
            status: PackStatus::Active,
            issues: Vec::new(),
        }
    }

    /// A pack that failed to load at all.
    #[must_use]
    pub(crate) fn failed(
        instance_id: PackInstanceId,
        path: PathBuf,
        manifest_name: Option<String>,
        component: PackComponent,
        error: &crate::error::Error,
    ) -> Self {
        let name = manifest_name.unwrap_or_else(|| {
            path.file_name()
                .and_then(|f| f.to_str())
                .unwrap_or("unknown")
                .to_owned()
        });
        Self {
            instance_id,
            name,
            path,
            status: PackStatus::Failed,
            issues: vec![PackIssue {
                component,
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

    /// Record an issue that fails the whole pack outright (#5208): a
    /// `required = true` tool failing registration, matching the escalation
    /// a `Priority::Required` context entry already gets at load. `Failed`
    /// is terminal and this only ever moves a pack toward it.
    pub(crate) fn fail_with(&mut self, issue: PackIssue) {
        self.status = PackStatus::Failed;
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
    /// A pack whose non-required tool fails to register stays active but
    /// degrades; a pack whose `required = true` tool fails is marked
    /// `failed` outright (#5208).
    pub fn record_tool_failures(&mut self, failures: &[PackToolFailure]) {
        for failure in failures {
            let issue = PackIssue {
                component: PackComponent::Tool,
                severity: Severity::Error,
                message: format!(
                    "tool '{}' failed to register: {}{}",
                    failure.tool_name,
                    failure.error,
                    if failure.required {
                        " (required tool -- pack failed)"
                    } else {
                        ""
                    }
                ),
            };
            if let Some(health) = self
                .packs
                .iter_mut()
                .find(|p| p.instance_id == failure.pack_instance_id)
            {
                if failure.required {
                    health.fail_with(issue);
                } else {
                    health.push_issue(issue);
                }
            } else {
                // WHY: registration only sees loaded packs, so a missing
                // entry means a loader/registration mismatch. Keep the
                // failure visible rather than dropping it silently.
                let mut health = PackHealth::active(
                    failure.pack_instance_id,
                    failure.pack_name.clone(),
                    PathBuf::new(),
                );
                if failure.required {
                    health.fail_with(issue);
                } else {
                    health.push_issue(issue);
                }
                self.packs.push(health);
            }
        }
    }
}

/// Host and configured capability notes for pack tool execution (#5215).
///
/// Notes come from the actual deployment sandbox plus Organon's preflight
/// guarantee probe. A disabled sandbox and every requested-but-reduced
/// guarantee are named instead of inferring safety from the host platform.
#[must_use]
pub fn platform_notes(sandbox: &organon::sandbox::SandboxConfig) -> Vec<String> {
    if !sandbox.enabled {
        let sandbox_note = "pack tool sandbox is disabled; filesystem, syscall, and egress \
                            sandbox restrictions are not applied"
            .to_owned();
        #[cfg(target_os = "linux")]
        return vec![sandbox_note];
        #[cfg(all(not(target_os = "linux"), unix))]
        return vec![
            sandbox_note,
            "subprocess resource limits (RLIMIT_NPROC, RLIMIT_CPU) are not enforced on this \
             platform; wall-clock timeout and Unix process-group cleanup still apply"
                .to_owned(),
        ];
        #[cfg(not(unix))]
        return vec![
            sandbox_note,
            "subprocess resource limits and process-group cleanup are unavailable on this \
             platform; wall-clock timeout still applies to the direct child"
                .to_owned(),
        ];
    }

    let guarantees = organon::sandbox::diagnostic_guarantees(sandbox);
    let mut notes = Vec::new();
    for (name, status) in [
        ("filesystem (Landlock)", guarantees.landlock),
        ("syscall (seccomp)", guarantees.seccomp),
        ("network egress", guarantees.egress),
    ] {
        if !matches!(
            status,
            organon::sandbox::GuaranteeStatus::Active
                | organon::sandbox::GuaranteeStatus::Unrestricted
        ) {
            notes.push(format!(
                "pack tool {name} guarantee is {status} under {:?} enforcement",
                sandbox.enforcement
            ));
        }
    }

    #[cfg(all(not(target_os = "linux"), unix))]
    notes.push(
        "subprocess resource limits (RLIMIT_NPROC, RLIMIT_CPU) are not enforced on this \
         platform; wall-clock timeout and Unix process-group cleanup still apply"
            .to_owned(),
    );

    #[cfg(not(unix))]
    notes.push(
        "subprocess resource limits and process-group cleanup are unavailable on this \
         platform; wall-clock timeout still applies to the direct child"
            .to_owned(),
    );

    notes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_pack_is_active() {
        let health = PackHealth::active(
            PackInstanceId::default(),
            "good".to_owned(),
            PathBuf::from("/packs/good"),
        );
        assert_eq!(health.status, PackStatus::Active);
        assert!(health.issues.is_empty());
    }

    #[test]
    fn warning_issue_degrades_active_pack() {
        let mut health = PackHealth::active(
            PackInstanceId::default(),
            "partial".to_owned(),
            PathBuf::from("/packs/partial"),
        );
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
        let mut health = PackHealth::active(
            PackInstanceId::default(),
            "noted".to_owned(),
            PathBuf::from("/packs/noted"),
        );
        health.push_issue(PackIssue {
            component: PackComponent::Overlay,
            severity: Severity::Info,
            message: "model override permitted by policy".to_owned(),
        });
        assert_eq!(health.status, PackStatus::Active);
    }

    #[test]
    fn failed_pack_stays_failed() {
        let err = crate::error::Error::ManifestNotFound {
            path: PathBuf::from("/packs/gone/pack.toml"),
            location: snafu::location!(),
        };
        let mut health = PackHealth::failed(
            PackInstanceId::default(),
            PathBuf::from("/packs/gone"),
            None,
            PackComponent::Manifest,
            &err,
        );
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
        report.packs.push(PackHealth::active(
            PackInstanceId::from_ordinal(0),
            "a".to_owned(),
            PathBuf::new(),
        ));
        let mut degraded = PackHealth::active(
            PackInstanceId::from_ordinal(1),
            "b".to_owned(),
            PathBuf::new(),
        );
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
        report.packs.push(PackHealth::failed(
            PackInstanceId::from_ordinal(2),
            PathBuf::from("/packs/c"),
            None,
            PackComponent::Manifest,
            &err,
        ));

        let counts = report.status_counts();
        assert_eq!(counts.active, 1);
        assert_eq!(counts.degraded, 1);
        assert_eq!(counts.failed, 1);
        assert!(report.has_failures());
    }

    #[test]
    fn disabled_sandbox_is_reported_explicitly() {
        let sandbox = organon::sandbox::SandboxConfig {
            enabled: false,
            ..organon::sandbox::SandboxConfig::default()
        };
        let notes = platform_notes(&sandbox);
        assert!(!notes.is_empty());
        assert!(
            notes
                .iter()
                .any(|note| note.contains("sandbox is disabled"))
        );
    }

    #[test]
    fn default_permissive_seccomp_reduction_is_reported_explicitly() {
        let sandbox = organon::sandbox::SandboxConfig::default();
        assert_eq!(
            sandbox.enforcement,
            organon::sandbox::SandboxEnforcement::Permissive
        );

        let notes = platform_notes(&sandbox);
        assert!(
            notes.iter().any(|note| {
                note.contains("syscall (seccomp)")
                    && note.contains("degraded")
                    && note.contains("Permissive")
            }),
            "startup notes must disclose log-only permissive seccomp: {notes:?}"
        );
    }
}
