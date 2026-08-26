//! Pack loading and context resolution.

use std::io::Read;
use std::path::{Path, PathBuf};

use snafu::ResultExt;
use tracing::{info, warn};

use crate::error::{self, Result};
use crate::health::{PackComponent, PackHealth, PackInstanceId, PackIssue, PackReport, Severity};
use crate::manifest::{self, ContextEntry, OverlayPolicy, PackManifest, Priority};

/// Maximum bytes read for a single context file.
///
/// WHY: Bounds per-file read to prevent a malicious or misconfigured pack from
/// causing unbounded heap growth or startup OOM.
pub const MAX_CONTEXT_FILE_BYTES: usize = 512 * 1024;

/// A resolved context section from a domain pack, ready for bootstrap injection.
#[derive(Debug, Clone)]
pub struct PackSection {
    /// Section name (derived from filename, e.g. `BUSINESS_LOGIC.md`).
    pub name: String,
    /// The text content.
    pub content: String,
    /// Bootstrap priority level.
    pub priority: Priority,
    /// Whether this section can be truncated under budget pressure.
    pub truncatable: bool,
    /// Optional agent filter. Empty = available to all agents.
    pub agents: Vec<String>,
    /// Which pack this section came from.
    pub pack_name: String,
}

/// A fully loaded domain pack with resolved context.
///
/// Its effective fields are private by design: callers may inspect a pack,
/// but only this loader can construct or mutate one after manifest validation,
/// context resolution, and operator-policy admission.
#[derive(Debug, Clone)]
pub struct LoadedPack {
    /// Stable identity of this configured pack occurrence.
    instance_id: PackInstanceId,
    /// The pack manifest.
    manifest: PackManifest,
    /// Resolved context sections with file contents read.
    sections: Vec<PackSection>,
    /// Absolute path to the pack root.
    root: PathBuf,
}

impl LoadedPack {
    /// Construct a pack from test-owned parts without exposing a production
    /// policy bypass to downstream crates.
    #[cfg(test)]
    pub(crate) fn for_test(
        instance_id: PackInstanceId,
        manifest: PackManifest,
        sections: Vec<PackSection>,
        root: PathBuf,
    ) -> Self {
        Self {
            instance_id,
            manifest,
            sections,
            root,
        }
    }

    /// Stable identity of this configured pack occurrence.
    #[must_use]
    pub const fn instance_id(&self) -> PackInstanceId {
        self.instance_id
    }

    /// Validated, policy-admitted manifest.
    #[must_use]
    pub const fn manifest(&self) -> &PackManifest {
        &self.manifest
    }

    /// Resolved context sections retained by loader policy.
    #[must_use]
    pub fn sections(&self) -> &[PackSection] {
        &self.sections
    }

    /// Configured pack root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Validated pack name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.manifest.name
    }

    /// Filter sections for an agent by ID or domain tags.
    ///
    /// A section matches if its `agents` list is empty (all agents),
    /// contains the agent ID, or contains any of the agent's domains.
    #[must_use]
    pub fn sections_for_agent_or_domains(
        &self,
        agent_id: &str,
        domains: &[String],
    ) -> Vec<&PackSection> {
        self.sections
            .iter()
            .filter(|s| {
                s.agents.is_empty()
                    || s.agents.iter().any(|a| a == agent_id)
                    || s.agents.iter().any(|a| domains.contains(a))
            })
            .collect()
    }

    /// Domain overlays for a specific agent, if any.
    #[must_use]
    pub fn domains_for_agent(&self, agent_id: &str) -> Vec<String> {
        self.manifest
            .overlays
            .get(agent_id)
            .map(|o| o.domains.clone())
            .unwrap_or_default()
    }

    /// Model override for a specific agent, if any.
    #[must_use]
    pub fn model_for_agent(&self, agent_id: &str) -> Option<String> {
        self.manifest
            .overlays
            .get(agent_id)
            .and_then(|o| o.model.clone())
    }

    /// Agency override for a specific agent, if any.
    #[must_use]
    pub fn agency_for_agent(&self, agent_id: &str) -> Option<String> {
        self.manifest
            .overlays
            .get(agent_id)
            .and_then(|o| o.agency.clone())
    }

    /// System-prompt additions for a specific agent, if any.
    #[must_use]
    pub fn system_prompt_additions_for_agent(&self, agent_id: &str) -> Vec<String> {
        self.manifest
            .overlays
            .get(agent_id)
            .map(|o| o.system_prompt_additions.clone())
            .unwrap_or_default()
    }
}

/// Outcome of loading every configured pack: the successfully loaded packs
/// plus a structured health record for all of them (#5208).
#[derive(Debug, Clone, Default)]
pub struct LoadOutcome {
    /// Packs that loaded and are active (possibly degraded).
    pub packs: Vec<LoadedPack>,
    /// Per-pack health, including packs that failed to load.
    pub report: PackReport,
}

/// Load all configured domain packs.
///
/// Reads manifests from each path, resolves context files, and returns loaded packs.
/// Invalid or missing packs emit warnings and are skipped (graceful degradation);
/// the per-pack detail of what was skipped is available through
/// [`load_packs_with_report`].
///
/// # Blocking I/O
///
/// This function performs synchronous file I/O and is intended to be called once
/// at startup, before the async runtime begins serving requests. If called from
/// within an async context during normal operation, wrap in
/// `tokio::task::spawn_blocking`.
pub fn load_packs(paths: &[PathBuf]) -> Vec<LoadedPack> {
    load_packs_with_report(paths).packs
}

/// Load all configured domain packs, returning a structured health report
/// alongside the successfully loaded packs.
///
/// Every configured path gets a [`PackHealth`] entry: `Active` when the pack
/// and all its context loaded, `Degraded` when it loaded with skips, and
/// `Failed` when the manifest or a required context entry failed.
///
/// High-impact overlay powers (model, agency, system-prompt additions) are
/// stripped under the restrictive default [`OverlayPolicy`]; use
/// [`load_packs_with_policy`] to apply the operator's configured policy.
pub fn load_packs_with_report(paths: &[PathBuf]) -> LoadOutcome {
    load_packs_with_policy(paths, &OverlayPolicy::default())
}

/// Load all configured domain packs under an explicit overlay policy
/// (#5220), returning the structured health report alongside the packs.
pub fn load_packs_with_policy(paths: &[PathBuf], policy: &OverlayPolicy) -> LoadOutcome {
    let mut packs = Vec::with_capacity(paths.len());
    let mut report = PackReport::default();

    for (ordinal, path) in paths.iter().enumerate() {
        let instance_id = PackInstanceId::from_ordinal(ordinal);
        match load_single_pack_inner(instance_id, path, policy) {
            Ok((pack, issues)) => {
                info!(
                    pack = %pack.manifest.name,
                    sections = pack.sections.len(),
                    path = %path.display(),
                    "domain pack loaded"
                );
                let mut health =
                    PackHealth::active(instance_id, pack.manifest.name.clone(), path.clone());
                for issue in issues {
                    health.push_issue(issue);
                }
                report.packs.push(health);
                packs.push(pack);
            }
            Err(failure) => {
                warn!(
                    path = %path.display(),
                    error = %failure.error,
                    "failed to load domain pack, skipping"
                );
                report.packs.push(PackHealth::failed(
                    instance_id,
                    path.clone(),
                    failure.manifest_name,
                    failure.component,
                    &failure.error,
                ));
            }
        }
    }

    if !packs.is_empty() {
        let total_sections: usize = packs.iter().map(|p| p.sections.len()).sum();
        info!(packs = packs.len(), total_sections, "domain packs loaded");
    }

    LoadOutcome { packs, report }
}

/// Load a single domain pack from a directory.
#[cfg(test)]
fn load_single_pack(pack_root: &Path) -> Result<LoadedPack> {
    load_single_pack_inner(
        PackInstanceId::default(),
        pack_root,
        &OverlayPolicy::default(),
    )
    .map(|(pack, _issues)| pack)
    .map_err(|failure| failure.error)
}

/// Typed internal load failure preserving how far pack loading progressed.
struct PackLoadFailure {
    manifest_name: Option<String>,
    component: PackComponent,
    error: error::Error,
}

/// Load a single domain pack, also returning health issues for every
/// non-fatal skip (missing optional context files, stripped overlay powers).
fn load_single_pack_inner(
    instance_id: PackInstanceId,
    pack_root: &Path,
    policy: &OverlayPolicy,
) -> std::result::Result<(LoadedPack, Vec<PackIssue>), PackLoadFailure> {
    let manifest = manifest::load_manifest(pack_root).map_err(|error| PackLoadFailure {
        manifest_name: None,
        component: PackComponent::Manifest,
        error,
    })?;
    let manifest_name = manifest.name.clone();
    let (sections, mut issues) =
        resolve_context_sections(pack_root, &manifest).map_err(|error| PackLoadFailure {
            manifest_name: Some(manifest_name),
            component: PackComponent::Context,
            error,
        })?;

    let mut pack = LoadedPack {
        instance_id,
        manifest,
        sections,
        root: pack_root.to_path_buf(),
    };
    issues.extend(apply_overlay_policy(&mut pack, policy));
    Ok((pack, issues))
}

/// Resolve all context entries into sections with file contents.
///
/// A resolution/read failure on a `Priority::Required` entry fails the whole
/// pack (propagated to the caller). Failures on any other priority are
/// logged, skipped, and recorded as health issues, so the pack still loads.
fn resolve_context_sections(
    pack_root: &Path,
    manifest: &PackManifest,
) -> Result<(Vec<PackSection>, Vec<PackIssue>)> {
    let mut sections = Vec::with_capacity(manifest.context.len());
    let mut issues = Vec::new();

    for entry in &manifest.context {
        match resolve_single_section(pack_root, entry, &manifest.name) {
            Ok(section) => sections.push(section),
            Err(e) if entry.priority == Priority::Required => return Err(e),
            Err(e) => {
                warn!(
                    path = %entry.path,
                    pack = %manifest.name,
                    error = %e,
                    "failed to resolve context file, skipping"
                );
                issues.push(PackIssue {
                    component: PackComponent::Context,
                    severity: Severity::Warning,
                    message: format!(
                        "context file '{}' skipped (priority {:?}): {e}",
                        entry.path, entry.priority
                    ),
                });
            }
        }
    }

    Ok((sections, issues))
}

/// Enforce the operator's overlay policy on a loaded pack (#5220).
///
/// High-impact powers (model override, agency override, durable
/// system-prompt additions) are stripped unless the operator opted in;
/// every permitted or stripped power is recorded as a health issue. Actual
/// runtime reconciliation occurs after agents and providers exist. Domain tags always apply — they only
/// route context. Prompt additions, when permitted, are capped at
/// `max_prompt_additions_bytes` per agent; additions past the cap are
/// dropped whole rather than truncated mid-string.
fn apply_overlay_policy(pack: &mut LoadedPack, policy: &OverlayPolicy) -> Vec<PackIssue> {
    let mut issues = Vec::new();

    // WHY: sorted iteration keeps the health record deterministic across
    // runs — HashMap order is randomized per process.
    let mut agents: Vec<String> = pack.manifest.overlays.keys().cloned().collect();
    agents.sort();

    for agent in agents {
        let Some(overlay) = pack.manifest.overlays.get_mut(&agent) else {
            continue;
        };

        if let Some(model) = overlay.model.take() {
            if policy.allow_model_overrides {
                issues.push(PackIssue {
                    component: PackComponent::Overlay,
                    severity: Severity::Info,
                    message: format!(
                        "overlay for agent '{agent}': model override '{model}' permitted and \
                         retained by operator policy"
                    ),
                });
                overlay.model = Some(model);
            } else {
                issues.push(PackIssue {
                    component: PackComponent::Overlay,
                    severity: Severity::Warning,
                    message: format!(
                        "overlay for agent '{agent}': model override '{model}' dropped — \
                         operator opt-in packOverlays.allowModelOverrides is not set"
                    ),
                });
            }
        }

        if let Some(agency) = overlay.agency.take() {
            if policy.allow_agency_overrides {
                issues.push(PackIssue {
                    component: PackComponent::Overlay,
                    severity: Severity::Info,
                    message: format!(
                        "overlay for agent '{agent}': agency override '{agency}' permitted and \
                         retained by operator policy"
                    ),
                });
                overlay.agency = Some(agency);
            } else {
                issues.push(PackIssue {
                    component: PackComponent::Overlay,
                    severity: Severity::Warning,
                    message: format!(
                        "overlay for agent '{agent}': agency override '{agency}' dropped — \
                         operator opt-in packOverlays.allowAgencyOverrides is not set"
                    ),
                });
            }
        }

        if overlay.system_prompt_additions.is_empty() {
            continue;
        }
        if !policy.allow_prompt_additions {
            let count = overlay.system_prompt_additions.len();
            overlay.system_prompt_additions.clear();
            issues.push(PackIssue {
                component: PackComponent::Overlay,
                severity: Severity::Warning,
                message: format!(
                    "overlay for agent '{agent}': {count} system-prompt addition(s) dropped — \
                     operator opt-in packOverlays.allowPromptAdditions is not set"
                ),
            });
            continue;
        }

        let cap = policy.max_prompt_additions_bytes;
        let mut kept = Vec::with_capacity(overlay.system_prompt_additions.len());
        let mut total = 0usize;
        let mut dropped = 0usize;
        for addition in std::mem::take(&mut overlay.system_prompt_additions) {
            if total.saturating_add(addition.len()) <= cap {
                total += addition.len();
                kept.push(addition);
            } else {
                dropped += 1;
            }
        }
        overlay.system_prompt_additions = kept;
        if dropped > 0 {
            issues.push(PackIssue {
                component: PackComponent::Overlay,
                severity: Severity::Warning,
                message: format!(
                    "overlay for agent '{agent}': {dropped} system-prompt addition(s) dropped \
                     over the {cap}-byte packOverlays.maxPromptAdditionBytes cap"
                ),
            });
        }
        issues.push(PackIssue {
            component: PackComponent::Overlay,
            severity: Severity::Info,
            message: format!(
                "overlay for agent '{agent}': {} system-prompt addition(s) permitted and retained \
                 by operator policy ({total} bytes)",
                overlay.system_prompt_additions.len()
            ),
        });
    }

    issues
}

/// Resolve a single context entry into a section.
fn resolve_single_section(
    pack_root: &Path,
    entry: &ContextEntry,
    pack_name: &str,
) -> Result<PackSection> {
    let file_path = manifest::resolve_context_path(pack_root, entry)?;

    #[expect(
        clippy::disallowed_methods,
        reason = "thesauros pack loader reads context files synchronously at startup; bounded synchronous I/O is inherent to asset loading"
    )]
    let mut file = std::fs::File::open(&file_path).context(error::ReadFileSnafu {
        path: file_path.clone(),
    })?;

    let mut content = String::new();
    #[expect(
        clippy::as_conversions,
        reason = "MAX_CONTEXT_FILE_BYTES is 512 KiB and always fits in u64"
    )]
    let byte_limit = MAX_CONTEXT_FILE_BYTES as u64;
    (&mut file)
        .take(byte_limit)
        .read_to_string(&mut content)
        .context(error::ReadFileSnafu {
            path: file_path.clone(),
        })?;

    // If the read consumed the whole budget, the file may be larger than the
    // limit. Read one more byte to confirm before keeping the content.
    if content.len() == MAX_CONTEXT_FILE_BYTES {
        let mut extra = [0u8; 1];
        match file.read(&mut extra) {
            Ok(0) => {}
            Ok(_) => {
                return Err(error::Error::ContextFileTooLarge {
                    path: file_path.clone(),
                    limit: MAX_CONTEXT_FILE_BYTES,
                    location: snafu::location!(),
                });
            }
            Err(source) => {
                return Err(error::Error::ReadFile {
                    path: file_path.clone(),
                    source,
                    location: snafu::location!(),
                });
            }
        }
    }

    let content = content.trim().to_owned();

    // SECURITY(#5220): bootstrap performs file-ref expansion only after it
    // combines every section and resolves references against the instance
    // root. Pack content is not authority to read that root. Reject the
    // marker here so a tiny context file cannot inject instance credentials
    // or expand beyond this loader's byte limit.
    if manifest::has_file_ref_interpolation(&content) {
        return Err(error::Error::ContextFileInterpolation {
            path: file_path,
            location: snafu::location!(),
        });
    }

    let name = file_path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("unknown")
        .to_owned();

    // A symlink may have a harmless declared path but a canonical target
    // whose filename carries the marker into the bootstrap section heading.
    if manifest::has_file_ref_interpolation(&name) {
        return Err(error::Error::ContextFileInterpolation {
            path: file_path,
            location: snafu::location!(),
        });
    }

    Ok(PackSection {
        name,
        content,
        priority: entry.priority,
        truncatable: entry.truncatable,
        agents: entry.agents.clone(),
        pack_name: pack_name.to_owned(),
    })
}

#[expect(
    clippy::indexing_slicing,
    reason = "test: vec indices are valid after asserting len"
)]
#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    fn setup_pack(files: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
        for (name, content) in files {
            let path = dir.path().join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            #[expect(
                clippy::disallowed_methods,
                reason = "thesauros pack loader reads binary assets from disk; synchronous I/O is inherent to asset loading"
            )]
            fs::write(&path, content).unwrap();
        }
        dir
    }

    fn full_pack_toml() -> &'static str {
        r#"
name = "test-pack"
version = "1.0"

[[context]]
path = "context/LOGIC.md"
priority = "important"
agents = ["analyst"]

[[context]]
path = "context/GLOSSARY.md"
priority = "flexible"
truncatable = true

[overlays.analyst]
domains = ["healthcare", "sql"]
"#
    }

    #[test]
    fn load_single_pack_succeeds() {
        let dir = setup_pack(&[
            ("pack.toml", full_pack_toml()),
            ("context/LOGIC.md", "Business logic content."),
            ("context/GLOSSARY.md", "Term definitions."),
        ]);

        let pack = load_single_pack(dir.path()).unwrap();
        assert_eq!(pack.manifest.name, "test-pack");
        assert_eq!(pack.sections.len(), 2);
        assert_eq!(pack.sections[0].name, "LOGIC.md");
        assert_eq!(pack.sections[0].content, "Business logic content.");
        assert_eq!(pack.sections[0].priority, Priority::Important);
        assert_eq!(pack.sections[0].agents, vec!["analyst"]);
        assert_eq!(pack.sections[0].pack_name, "test-pack");
        assert_eq!(pack.sections[1].name, "GLOSSARY.md");
        assert!(pack.sections[1].truncatable);
    }

    #[test]
    fn load_packs_multiple() {
        let dir1 = setup_pack(&[
            (
                "pack.toml",
                "name = \"pack-a\"\nversion = \"1.0\"\n\n[[context]]\npath = \"a.md\"\n",
            ),
            ("a.md", "Content A"),
        ]);
        let dir2 = setup_pack(&[
            (
                "pack.toml",
                "name = \"pack-b\"\nversion = \"1.0\"\n\n[[context]]\npath = \"b.md\"\n",
            ),
            ("b.md", "Content B"),
        ]);

        let packs = load_packs(&[dir1.path().to_path_buf(), dir2.path().to_path_buf()]);
        assert_eq!(packs.len(), 2);
        assert_eq!(packs[0].manifest.name, "pack-a");
        assert_eq!(packs[1].manifest.name, "pack-b");
    }

    #[test]
    fn load_packs_skips_invalid() {
        let good = setup_pack(&[("pack.toml", "name = \"good\"\nversion = \"1.0\"\n")]);

        let packs = load_packs(&[
            PathBuf::from("/nonexistent/pack"),
            good.path().to_path_buf(),
        ]);
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].manifest.name, "good");
    }

    #[test]
    fn load_packs_empty_paths() {
        let packs = load_packs(&[]);
        assert!(packs.is_empty());
    }

    #[test]
    fn sections_for_agent_or_domains_by_agent() {
        let dir = setup_pack(&[
            ("pack.toml", full_pack_toml()),
            ("context/LOGIC.md", "logic"),
            ("context/GLOSSARY.md", "glossary"),
        ]);

        let pack = load_single_pack(dir.path()).unwrap();

        let sections = pack.sections_for_agent_or_domains("analyst", &[]);
        assert_eq!(sections.len(), 2);
    }

    #[test]
    fn sections_for_agent_or_domains_by_domain() {
        let toml = r#"
name = "domain-test"
version = "1.0"

[[context]]
path = "general.md"

[[context]]
path = "healthcare.md"
agents = ["healthcare"]

[[context]]
path = "sql.md"
agents = ["sql"]
"#;
        let dir = setup_pack(&[
            ("pack.toml", toml),
            ("general.md", "general content"),
            ("healthcare.md", "healthcare content"),
            ("sql.md", "sql content"),
        ]);

        let pack = load_single_pack(dir.path()).unwrap();

        let sections = pack.sections_for_agent_or_domains("hermes", &["healthcare".to_owned()]);
        assert_eq!(sections.len(), 2);
        let names: Vec<&str> = sections.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"general.md"));
        assert!(names.contains(&"healthcare.md"));
    }

    #[test]
    fn sections_for_agent_or_domains_no_match() {
        let toml = r#"
name = "filter-test"
version = "1.0"

[[context]]
path = "general.md"

[[context]]
path = "restricted.md"
agents = ["analyst"]
"#;
        let dir = setup_pack(&[
            ("pack.toml", toml),
            ("general.md", "general"),
            ("restricted.md", "restricted"),
        ]);

        let pack = load_single_pack(dir.path()).unwrap();

        let sections = pack.sections_for_agent_or_domains("unknown", &["analytics".to_owned()]);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].name, "general.md");
    }

    #[test]
    fn domains_for_agent() {
        let dir = setup_pack(&[
            ("pack.toml", full_pack_toml()),
            ("context/LOGIC.md", "logic"),
            ("context/GLOSSARY.md", "glossary"),
        ]);

        let pack = load_single_pack(dir.path()).unwrap();
        assert_eq!(pack.domains_for_agent("analyst"), vec!["healthcare", "sql"]);
        assert!(pack.domains_for_agent("hermes").is_empty());
    }

    #[test]
    fn overlay_fields_for_agent() {
        let toml = r#"
name = "overlay-test"
version = "1.0"

[overlays.analyst]
domains = ["healthcare"]
model = "anubis-70b"
agency = "unrestricted"
system_prompt_additions = ["Answer in bullet points."]
"#;
        let dir = setup_pack(&[("pack.toml", toml)]);

        // WHY(#5220): high-impact overlay powers require operator opt-in;
        // this test exercises the permitted path via an explicit policy.
        let outcome =
            load_packs_with_policy(&[dir.path().to_path_buf()], &OverlayPolicy::permit_all());
        let pack = &outcome.packs[0];
        assert_eq!(pack.domains_for_agent("analyst"), vec!["healthcare"]);
        assert_eq!(
            pack.model_for_agent("analyst"),
            Some("anubis-70b".to_owned())
        );
        assert_eq!(
            pack.agency_for_agent("analyst"),
            Some("unrestricted".to_owned())
        );
        assert_eq!(
            pack.system_prompt_additions_for_agent("analyst"),
            vec!["Answer in bullet points.".to_owned()]
        );

        // Empty overlay for unknown agents
        assert!(pack.domains_for_agent("hermes").is_empty());
        assert_eq!(pack.model_for_agent("hermes"), None);
        assert_eq!(pack.agency_for_agent("hermes"), None);
        assert!(pack.system_prompt_additions_for_agent("hermes").is_empty());
    }

    #[test]
    fn default_policy_strips_high_impact_overlay_powers() {
        // WHY(#5220): without operator opt-in, a pack must not change the
        // model, raise agency limits, or inject durable prompt text.
        let toml = r#"
name = "overlay-strict"
version = "1.0"

[overlays.analyst]
domains = ["healthcare"]
model = "anubis-70b"
agency = "unrestricted"
system_prompt_additions = ["Answer in bullet points."]
"#;
        let dir = setup_pack(&[("pack.toml", toml)]);

        let outcome = load_packs_with_report(&[dir.path().to_path_buf()]);
        let pack = &outcome.packs[0];
        assert_eq!(pack.domains_for_agent("analyst"), vec!["healthcare"]);
        assert_eq!(pack.model_for_agent("analyst"), None);
        assert_eq!(pack.agency_for_agent("analyst"), None);
        assert!(pack.system_prompt_additions_for_agent("analyst").is_empty());

        let health = &outcome.report.packs[0];
        assert_eq!(health.status, crate::health::PackStatus::Degraded);
        assert_eq!(health.issues.len(), 3, "one note per stripped power");
        for issue in &health.issues {
            assert_eq!(issue.component, crate::health::PackComponent::Overlay);
            assert_eq!(issue.severity, crate::health::Severity::Warning);
            assert!(issue.message.contains("dropped"), "{issue:?}");
        }
    }

    #[test]
    fn permit_all_records_applied_powers_without_degrading() {
        let toml = r#"
name = "overlay-permitted"
version = "1.0"

[overlays.analyst]
model = "anubis-70b"
agency = "standard"
system_prompt_additions = ["Cite sources."]
"#;
        let dir = setup_pack(&[("pack.toml", toml)]);

        let outcome =
            load_packs_with_policy(&[dir.path().to_path_buf()], &OverlayPolicy::permit_all());
        let health = &outcome.report.packs[0];
        assert_eq!(
            health.status,
            crate::health::PackStatus::Active,
            "applied-with-opt-in powers are Info notes, not degradation: {:?}",
            health.issues
        );
        assert_eq!(health.issues.len(), 3);
        assert!(
            health
                .issues
                .iter()
                .all(|i| i.severity == crate::health::Severity::Info)
        );
    }

    #[test]
    fn prompt_additions_capped_per_agent() {
        let toml = r#"
name = "overlay-capped"
version = "1.0"

[overlays.analyst]
system_prompt_additions = ["aaaaa", "bbbbb", "c"]
"#;
        let dir = setup_pack(&[("pack.toml", toml)]);

        let policy = OverlayPolicy {
            max_prompt_additions_bytes: 10,
            ..OverlayPolicy::permit_all()
        };
        let outcome = load_packs_with_policy(&[dir.path().to_path_buf()], &policy);
        let pack = &outcome.packs[0];
        assert_eq!(
            pack.system_prompt_additions_for_agent("analyst"),
            vec!["aaaaa".to_owned(), "bbbbb".to_owned()],
            "the 11th byte must not fit; additions drop whole, never mid-string"
        );
        let health = &outcome.report.packs[0];
        assert!(
            health.issues.iter().any(
                |i| i.severity == crate::health::Severity::Warning && i.message.contains("cap")
            ),
            "cap drop must be recorded: {:?}",
            health.issues
        );
    }

    #[test]
    fn prompt_addition_cap_is_independent_per_pack_and_agent() {
        let first = setup_pack(&[(
            "pack.toml",
            "name = \"first\"\nversion = \"1.0\"\n\n[overlays.analyst]\n\
             system_prompt_additions = [\"aaaaa\"]\n",
        )]);
        let second = setup_pack(&[(
            "pack.toml",
            "name = \"second\"\nversion = \"1.0\"\n\n[overlays.analyst]\n\
             system_prompt_additions = [\"bbbbb\"]\n",
        )]);
        let policy = OverlayPolicy {
            max_prompt_additions_bytes: 5,
            ..OverlayPolicy::permit_all()
        };

        let outcome = load_packs_with_policy(
            &[first.path().to_path_buf(), second.path().to_path_buf()],
            &policy,
        );

        assert_eq!(outcome.packs.len(), 2);
        assert_eq!(
            outcome.packs[0].system_prompt_additions_for_agent("analyst"),
            vec!["aaaaa".to_owned()]
        );
        assert_eq!(
            outcome.packs[1].system_prompt_additions_for_agent("analyst"),
            vec!["bbbbb".to_owned()],
            "the second configured pack gets its own per-pack/per-agent cap"
        );
    }

    #[test]
    fn missing_context_file_skipped_gracefully() {
        let toml = "name = \"partial\"\nversion = \"1.0\"\n\n[[context]]\npath = \"exists.md\"\n\n[[context]]\npath = \"missing.md\"\n";
        let dir = setup_pack(&[("pack.toml", toml), ("exists.md", "content")]);

        let pack = load_single_pack(dir.path()).unwrap();
        assert_eq!(pack.sections.len(), 1);
        assert_eq!(pack.sections[0].name, "exists.md");
    }

    #[test]
    fn missing_required_context_file_fails_the_pack() {
        // WHY(#5206): a Required context entry that cannot resolve must fail the
        // whole pack, not be silently warn+skipped like lower priorities.
        let toml = "name = \"strict\"\nversion = \"1.0\"\n\n[[context]]\npath = \"missing.md\"\npriority = \"required\"\n";
        let dir = setup_pack(&[("pack.toml", toml)]);

        assert!(
            load_single_pack(dir.path()).is_err(),
            "a missing Required context file must fail the whole pack load"
        );
    }

    #[test]
    fn context_file_size_limit_rejects_oversized_file() {
        let oversized = "a".repeat(MAX_CONTEXT_FILE_BYTES + 1);
        let dir = setup_pack(&[
            (
                "pack.toml",
                "name = \"size-test\"\nversion = \"1.0\"\n\n[[context]]\npath = \"huge.md\"\n",
            ),
            ("huge.md", &oversized),
        ]);

        let entry = ContextEntry {
            path: "huge.md".to_owned(),
            priority: Priority::Important,
            agents: vec![],
            truncatable: false,
        };
        let err = resolve_single_section(dir.path(), &entry, "size-test").unwrap_err();
        assert!(
            matches!(err, error::Error::ContextFileTooLarge { .. }),
            "expected ContextFileTooLarge, got: {err}"
        );
    }

    #[test]
    fn context_file_at_size_limit_is_accepted() {
        let at_limit = "a".repeat(MAX_CONTEXT_FILE_BYTES);
        let dir = setup_pack(&[
            (
                "pack.toml",
                "name = \"size-test\"\nversion = \"1.0\"\n\n[[context]]\npath = \"limit.md\"\n",
            ),
            ("limit.md", &at_limit),
        ]);

        let pack = load_single_pack(dir.path()).unwrap();
        assert_eq!(pack.sections.len(), 1);
        assert_eq!(pack.sections[0].content.len(), MAX_CONTEXT_FILE_BYTES);
    }

    #[test]
    fn context_file_interpolation_is_rejected_at_loader_boundary() {
        // SECURITY(#5220): a tiny pack context file must not delegate a read
        // to the later instance-root interpolation pass and thereby bypass
        // this loader's 512 KiB bound.
        let dir = setup_pack(&[
            (
                "pack.toml",
                "name = \"interp-test\"\nversion = \"1.0\"\n\n[[context]]\npath = \"read.md\"\n",
            ),
            ("read.md", "{{file:config/env}}"),
        ]);
        let entry = ContextEntry {
            path: "read.md".to_owned(),
            priority: Priority::Important,
            agents: vec![],
            truncatable: false,
        };

        let err = resolve_single_section(dir.path(), &entry, "interp-test")
            .expect_err("pack context must not cross the trusted file-ref boundary");
        assert!(
            matches!(err, error::Error::ContextFileInterpolation { .. }),
            "expected ContextFileInterpolation, got: {err}"
        );
    }

    #[test]
    fn content_is_trimmed() {
        let dir = setup_pack(&[
            (
                "pack.toml",
                "name = \"trim-test\"\nversion = \"1.0\"\n\n[[context]]\npath = \"padded.md\"\n",
            ),
            ("padded.md", "\n\n  Content with whitespace.  \n\n"),
        ]);

        let pack = load_single_pack(dir.path()).unwrap();
        assert_eq!(pack.sections[0].content, "Content with whitespace.");
    }

    #[test]
    fn pack_root_stored() {
        let dir = setup_pack(&[("pack.toml", "name = \"root-test\"\nversion = \"1.0\"\n")]);
        let pack = load_single_pack(dir.path()).unwrap();
        assert_eq!(pack.root, dir.path());
    }

    #[test]
    fn report_marks_clean_pack_active() {
        let dir = setup_pack(&[
            ("pack.toml", full_pack_toml()),
            ("context/LOGIC.md", "logic"),
            ("context/GLOSSARY.md", "glossary"),
        ]);

        let outcome = load_packs_with_report(&[dir.path().to_path_buf()]);
        assert_eq!(outcome.packs.len(), 1);
        assert_eq!(outcome.report.packs.len(), 1);
        let health = &outcome.report.packs[0];
        assert_eq!(health.name, "test-pack");
        assert_eq!(health.status, crate::health::PackStatus::Active);
        assert!(health.issues.is_empty());
        assert!(!outcome.report.has_failures());
    }

    #[test]
    fn report_marks_pack_with_skipped_optional_context_degraded() {
        let toml = "name = \"partial\"\nversion = \"1.0\"\n\n[[context]]\npath = \"exists.md\"\n\n[[context]]\npath = \"missing.md\"\n";
        let dir = setup_pack(&[("pack.toml", toml), ("exists.md", "content")]);

        let outcome = load_packs_with_report(&[dir.path().to_path_buf()]);
        assert_eq!(outcome.packs.len(), 1, "pack still loads");
        let health = &outcome.report.packs[0];
        assert_eq!(health.status, crate::health::PackStatus::Degraded);
        assert_eq!(health.issues.len(), 1);
        assert_eq!(
            health.issues[0].component,
            crate::health::PackComponent::Context
        );
        assert!(
            health.issues[0].message.contains("missing.md"),
            "issue should name the skipped file: {}",
            health.issues[0].message
        );
    }

    #[test]
    fn report_marks_pack_with_failed_required_context_failed() {
        let toml = "name = \"strict\"\nversion = \"1.0\"\n\n[[context]]\npath = \"missing.md\"\npriority = \"required\"\n";
        let dir = setup_pack(&[("pack.toml", toml)]);

        let outcome = load_packs_with_report(&[dir.path().to_path_buf()]);
        assert!(outcome.packs.is_empty(), "pack must not activate");
        assert_eq!(outcome.report.packs.len(), 1);
        assert_eq!(
            outcome.report.packs[0].status,
            crate::health::PackStatus::Failed
        );
        assert_eq!(
            outcome.report.packs[0].name, "strict",
            "the parsed manifest name survives a later context-stage failure"
        );
        assert_eq!(
            outcome.report.packs[0].issues[0].component,
            crate::health::PackComponent::Context
        );
        assert!(outcome.report.has_failures());
    }

    #[test]
    fn report_records_invalid_manifest_as_failed() {
        let dir = setup_pack(&[("pack.toml", "name = \"bad name!\"\nversion = \"1.0\"\n")]);

        let outcome = load_packs_with_report(&[dir.path().to_path_buf()]);
        assert!(outcome.packs.is_empty());
        assert_eq!(outcome.report.packs.len(), 1);
        let health = &outcome.report.packs[0];
        assert_eq!(health.status, crate::health::PackStatus::Failed);
        assert_eq!(
            health.issues[0].component,
            crate::health::PackComponent::Manifest
        );
        assert_eq!(
            health.name,
            dir.path()
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap(),
            "an invalid manifest falls back to its configured path name"
        );
    }

    #[test]
    fn configured_occurrences_receive_distinct_stable_ids() {
        let dir = setup_pack(&[("pack.toml", "name = \"same\"\nversion = \"1.0\"\n")]);
        let path = dir.path().to_path_buf();
        let outcome = load_packs_with_report(&[path.clone(), path]);

        assert_eq!(outcome.packs.len(), 2);
        assert_eq!(outcome.report.packs.len(), 2);
        assert_eq!(outcome.packs[0].instance_id.ordinal(), 0);
        assert_eq!(outcome.packs[1].instance_id.ordinal(), 1);
        assert_eq!(outcome.report.packs[0].instance_id.ordinal(), 0);
        assert_eq!(outcome.report.packs[1].instance_id.ordinal(), 1);
    }
}
