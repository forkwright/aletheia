//! Pack loading and context resolution.

use std::path::{Path, PathBuf};

use snafu::ResultExt;
use tracing::{info, warn};

use crate::error::{self, Result};
use crate::manifest::{self, ContextEntry, PackManifest, Priority};

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
#[derive(Debug, Clone)]
pub struct LoadedPack {
    /// The pack manifest.
    pub manifest: PackManifest,
    /// Resolved context sections with file contents read.
    pub sections: Vec<PackSection>,
    /// Absolute path to the pack root.
    pub root: PathBuf,
}

impl LoadedPack {
    /// Filter sections for a specific agent.
    ///
    /// Returns sections where the agent filter is empty (all agents)
    /// or contains the given agent id.
    #[must_use]
    pub fn sections_for_agent(&self, agent_id: &str) -> Vec<&PackSection> {
        self.sections
            .iter()
            .filter(|s| s.agents.is_empty() || s.agents.iter().any(|a| a == agent_id))
            .collect()
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
}

/// Load all configured domain packs.
///
/// Reads manifests from each path, resolves context files, and returns loaded packs.
/// Invalid or missing packs emit warnings and are skipped (graceful degradation).
///
/// # Blocking I/O
///
/// This function performs synchronous file I/O and is intended to be called once
/// at startup, before the async runtime begins serving requests. If called from
/// within an async context during normal operation, wrap in
/// `tokio::task::spawn_blocking`.
pub fn load_packs(paths: &[PathBuf]) -> Vec<LoadedPack> {
    let mut packs = Vec::with_capacity(paths.len());

    for path in paths {
        match load_single_pack(path) {
            Ok(pack) => {
                info!(
                    pack = %pack.manifest.name,
                    sections = pack.sections.len(),
                    path = %path.display(),
                    "domain pack loaded"
                );
                packs.push(pack);
            }
            Err(e) => {
                warn!(
                    path = %path.display(),
                    error = %e,
                    "failed to load domain pack, skipping"
                );
            }
        }
    }

    if !packs.is_empty() {
        let total_sections: usize = packs.iter().map(|p| p.sections.len()).sum();
        info!(packs = packs.len(), total_sections, "domain packs loaded");
    }

    packs
}

/// Load a single domain pack from a directory.
fn load_single_pack(pack_root: &Path) -> Result<LoadedPack> {
    let manifest = manifest::load_manifest(pack_root)?;
    let sections = resolve_context_sections(pack_root, &manifest);

    Ok(LoadedPack {
        manifest,
        sections,
        root: pack_root.to_path_buf(),
    })
}

/// Resolve all context entries into sections with file contents.
fn resolve_context_sections(pack_root: &Path, manifest: &PackManifest) -> Vec<PackSection> {
    let mut sections = Vec::with_capacity(manifest.context.len());

    for entry in &manifest.context {
        match resolve_single_section(pack_root, entry, &manifest.name) {
            Ok(section) => sections.push(section),
            Err(e) => {
                warn!(
                    path = %entry.path,
                    pack = %manifest.name,
                    error = %e,
                    "failed to resolve context file, skipping"
                );
            }
        }
    }

    sections
}

/// Resolve a single context entry into a section.
fn resolve_single_section(
    pack_root: &Path,
    entry: &ContextEntry,
    pack_name: &str,
) -> Result<PackSection> {
    let file_path = manifest::resolve_context_path(pack_root, entry)?;
    let content = std::fs::read_to_string(&file_path).context(error::ReadFileSnafu {
        path: file_path.clone(),
    })?;
    let content = content.trim().to_owned();

    let name = file_path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("unknown")
        .to_owned();

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
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_pack(files: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
        for (name, content) in files {
            let path = dir.path().join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
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
agents = ["chiron"]

[[context]]
path = "context/GLOSSARY.md"
priority = "flexible"
truncatable = true

[overlays.chiron]
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
        assert_eq!(pack.sections[0].agents, vec!["chiron"]);
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
    fn sections_for_agent_filters() {
        let dir = setup_pack(&[
            ("pack.toml", full_pack_toml()),
            ("context/LOGIC.md", "logic"),
            ("context/GLOSSARY.md", "glossary"),
        ]);

        let pack = load_single_pack(dir.path()).unwrap();

        let chiron_sections = pack.sections_for_agent("chiron");
        assert_eq!(chiron_sections.len(), 2);

        let hermes_sections = pack.sections_for_agent("hermes");
        assert_eq!(hermes_sections.len(), 1);
        assert_eq!(hermes_sections[0].name, "GLOSSARY.md");
    }

    #[test]
    fn sections_for_agent_or_domains_by_agent() {
        let dir = setup_pack(&[
            ("pack.toml", full_pack_toml()),
            ("context/LOGIC.md", "logic"),
            ("context/GLOSSARY.md", "glossary"),
        ]);

        let pack = load_single_pack(dir.path()).unwrap();

        let sections = pack.sections_for_agent_or_domains("chiron", &[]);
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
agents = ["chiron"]
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
        assert_eq!(pack.domains_for_agent("chiron"), vec!["healthcare", "sql"]);
        assert!(pack.domains_for_agent("hermes").is_empty());
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
}
