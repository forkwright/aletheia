//! Three-tier cascade resolution.
//!
//! Walks the oikos hierarchy to discover and resolve files:
//!   1. `instance/nous/{id}/{subdir}/`: agent-specific
//!   2. `instance/shared/{subdir}/`   : shared across all agents
//!   3. `instance/theke/{subdir}/`    : human + agent collaborative
//!
//! Most specific wins on name collision.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::PathBuf;

use tracing::debug;

use aletheia_koina::system::{FileSystem, RealSystem};

use crate::oikos::Oikos;

/// Which tier a resolved file came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Tier {
    /// Agent-specific (most specific).
    Nous,
    /// Shared across all agents.
    Shared,
    /// Human + agent collaborative (least specific).
    Theke,
}

impl std::fmt::Display for Tier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Nous => f.write_str("nous"),
            Self::Shared => f.write_str("shared"),
            Self::Theke => f.write_str("theke"),
        }
    }
}

/// A file discovered through the cascade.
#[derive(Debug, Clone)]
pub struct CascadeEntry {
    /// Absolute file path.
    pub path: PathBuf,
    /// Which tier it came from.
    pub tier: Tier,
    /// Filename (basename).
    pub name: String,
}

/// Walk the three-tier cascade and discover files in a subdirectory.
///
/// When a filename exists in multiple tiers, only the most-specific version
/// is returned (nous > shared > theke).
///
/// Call [`discover_with`] to supply a custom [`FileSystem`] implementation
/// (e.g. [`aletheia_koina::system::TestSystem`] in tests).
///
/// # Arguments
/// * `oikos`: The oikos instance for path resolution
/// * `nous_id`: Agent ID for tier-1 resolution
/// * `subdir`: Subdirectory name (e.g. "tools", "hooks", "templates")
/// * `ext`: Optional extension filter (e.g. "md", "yaml"). Without the dot.
#[must_use]
pub fn discover(
    oikos: &Oikos,
    nous_id: &str,
    subdir: &str,
    ext: Option<&str>,
) -> Vec<CascadeEntry> {
    discover_with(&RealSystem, oikos, nous_id, subdir, ext)
}

/// Walk the three-tier cascade using the provided [`FileSystem`].
///
/// This is the primary implementation; [`discover`] is a convenience wrapper
/// that passes [`RealSystem`]. Prefer this variant in tests so that virtual
/// tier directories can be populated in-memory.
///
/// # Arguments
/// * `fs`: Filesystem implementation to use for listing and querying files
/// * `oikos`: The oikos instance for path resolution
/// * `nous_id`: Agent ID for tier-1 resolution
/// * `subdir`: Subdirectory name (e.g. "tools", "hooks", "templates")
/// * `ext`: Optional extension filter (e.g. "md", "yaml"). Without the dot.
#[must_use]
pub fn discover_with(
    fs: &impl FileSystem,
    oikos: &Oikos,
    nous_id: &str,
    subdir: &str,
    ext: Option<&str>,
) -> Vec<CascadeEntry> {
    let tiers = [
        (Tier::Nous, oikos.nous_dir(nous_id).join(subdir)),
        (Tier::Shared, oikos.shared().join(subdir)),
        (Tier::Theke, oikos.theke().join(subdir)),
    ];

    let mut seen: HashMap<String, CascadeEntry> = HashMap::new();

    for (tier, dir) in &tiers {
        let Ok(entries) = fs.list_dir(dir) else {
            continue;
        };

        for path in entries {
            if !fs.is_file(&path) {
                continue;
            }
            let name = match path.file_name().and_then(OsStr::to_str) {
                Some(n) if !n.starts_with('.') => n.to_owned(),
                _ => continue,
            };

            if let Some(required_ext) = ext {
                match path.extension().and_then(OsStr::to_str) {
                    // NOTE: extension matches, fall through to insertion
                    Some(e) if e == required_ext => {}
                    _ => continue,
                }
            }

            // WHY: most specific wins: only insert if not already seen
            seen.entry(name.clone()).or_insert_with(|| CascadeEntry {
                path,
                tier: *tier,
                name,
            });
        }
    }

    let results: Vec<CascadeEntry> = seen.into_values().collect();

    if !results.is_empty() {
        let nous_count = results.iter().filter(|r| r.tier == Tier::Nous).count();
        let shared_count = results.iter().filter(|r| r.tier == Tier::Shared).count();
        let theke_count = results.iter().filter(|r| r.tier == Tier::Theke).count();
        debug!(
            subdir,
            nous_id,
            total = results.len(),
            nous_count,
            shared_count,
            theke_count,
            "cascade discover"
        );
    }

    results
}

/// Resolve a single named file through the cascade.
///
/// Returns the most-specific path, or `None` if not found in any tier.
/// Call [`resolve_with`] to supply a custom [`FileSystem`].
#[must_use]
pub fn resolve(
    oikos: &Oikos,
    nous_id: &str,
    filename: &str,
    subdir: Option<&str>,
) -> Option<PathBuf> {
    resolve_with(&RealSystem, oikos, nous_id, filename, subdir)
}

/// Resolve a single named file using the provided [`FileSystem`].
///
/// Returns the most-specific path (nous > shared > theke), or `None`.
#[must_use]
pub fn resolve_with(
    fs: &impl FileSystem,
    oikos: &Oikos,
    nous_id: &str,
    filename: &str,
    subdir: Option<&str>,
) -> Option<PathBuf> {
    let candidates: Vec<PathBuf> = if let Some(sub) = subdir {
        vec![
            oikos.nous_dir(nous_id).join(sub).join(filename),
            oikos.shared().join(sub).join(filename),
            oikos.theke().join(sub).join(filename),
        ]
    } else {
        vec![
            oikos.nous_dir(nous_id).join(filename),
            oikos.shared().join(filename),
            oikos.theke().join(filename),
        ]
    };

    for candidate in candidates {
        if fs.exists(&candidate) {
            debug!(?candidate, filename, "cascade resolved");
            return Some(candidate);
        }
    }

    None
}

/// Resolve all instances of a named file across all tiers.
///
/// Returns matches ordered most-specific first. Useful for config deep-merge
/// where all tiers contribute. Call [`resolve_all_with`] to supply a custom
/// [`FileSystem`].
#[must_use]
pub fn resolve_all(
    oikos: &Oikos,
    nous_id: &str,
    filename: &str,
    subdir: Option<&str>,
) -> Vec<CascadeEntry> {
    resolve_all_with(&RealSystem, oikos, nous_id, filename, subdir)
}

/// Resolve all instances of a named file using the provided [`FileSystem`].
///
/// Returns matches ordered most-specific first (nous > shared > theke).
#[must_use]
pub fn resolve_all_with(
    fs: &impl FileSystem,
    oikos: &Oikos,
    nous_id: &str,
    filename: &str,
    subdir: Option<&str>,
) -> Vec<CascadeEntry> {
    let tiers: Vec<(Tier, PathBuf)> = if let Some(sub) = subdir {
        vec![
            (Tier::Nous, oikos.nous_dir(nous_id).join(sub).join(filename)),
            (Tier::Shared, oikos.shared().join(sub).join(filename)),
            (Tier::Theke, oikos.theke().join(sub).join(filename)),
        ]
    } else {
        vec![
            (Tier::Nous, oikos.nous_dir(nous_id).join(filename)),
            (Tier::Shared, oikos.shared().join(filename)),
            (Tier::Theke, oikos.theke().join(filename)),
        ]
    };

    tiers
        .into_iter()
        .filter(|(_, path)| fs.exists(path))
        .map(|(tier, path)| CascadeEntry {
            path,
            tier,
            name: filename.to_owned(),
        })
        .collect()
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: index 0 is valid after asserting results.len() >= 1"
)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::*;

    fn setup_oikos() -> (tempfile::TempDir, Oikos) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let oikos = Oikos::from_root(dir.path());
        (dir, oikos)
    }

    fn mkfile(base: &Path, rel: &str) {
        let path = base.join(rel);
        fs::create_dir_all(path.parent().unwrap()).expect("create parent dirs");
        #[expect(
            clippy::disallowed_methods,
            reason = "taxis config operations are CLI-invoked and require synchronous filesystem access"
        )]
        fs::write(&path, format!("content of {rel}")).expect("write file");
    }

    #[test]
    fn discovers_from_all_tiers() {
        let (dir, oikos) = setup_oikos();
        mkfile(dir.path(), "nous/syn/tools/agent-only.md");
        mkfile(dir.path(), "shared/tools/shared-tool.md");
        mkfile(dir.path(), "theke/tools/theke-tool.md");

        let results = discover(&oikos, "syn", "tools", Some("md"));
        assert_eq!(
            results.len(),
            3,
            "should discover files from all three tiers"
        );

        let names: Vec<&str> = results.iter().map(|r| r.name.as_str()).collect();
        assert!(
            names.contains(&"agent-only.md"),
            "should include nous-tier file"
        );
        assert!(
            names.contains(&"shared-tool.md"),
            "should include shared-tier file"
        );
        assert!(
            names.contains(&"theke-tool.md"),
            "should include theke-tier file"
        );
    }

    #[test]
    fn most_specific_wins() {
        let (dir, oikos) = setup_oikos();
        mkfile(dir.path(), "nous/syn/tools/override.md");
        mkfile(dir.path(), "shared/tools/override.md");
        mkfile(dir.path(), "theke/tools/override.md");

        let results = discover(&oikos, "syn", "tools", Some("md"));
        assert_eq!(
            results.len(),
            1,
            "duplicate filename should resolve to single entry"
        );
        assert_eq!(
            results[0].tier,
            Tier::Nous,
            "nous tier should win over shared and theke"
        );
    }

    #[test]
    fn shared_overrides_theke() {
        let (dir, oikos) = setup_oikos();
        mkfile(dir.path(), "shared/hooks/common.md");
        mkfile(dir.path(), "theke/hooks/common.md");

        let results = discover(&oikos, "syn", "hooks", Some("md"));
        assert_eq!(
            results.len(),
            1,
            "duplicate filename should resolve to single entry"
        );
        assert_eq!(
            results[0].tier,
            Tier::Shared,
            "shared tier should win over theke"
        );
    }

    #[test]
    fn extension_filter() {
        let (dir, oikos) = setup_oikos();
        mkfile(dir.path(), "shared/tools/tool.md");
        mkfile(dir.path(), "shared/tools/tool.yaml");

        let md = discover(&oikos, "syn", "tools", Some("md"));
        assert_eq!(md.len(), 1, "md filter should match exactly one file");
        assert_eq!(
            md[0].name, "tool.md",
            "md filter should return the .md file"
        );

        let yaml = discover(&oikos, "syn", "tools", Some("yaml"));
        assert_eq!(yaml.len(), 1, "yaml filter should match exactly one file");
        assert_eq!(
            yaml[0].name, "tool.yaml",
            "yaml filter should return the .yaml file"
        );
    }

    #[test]
    fn missing_dirs_return_empty() {
        let (_dir, oikos) = setup_oikos();
        let results = discover(&oikos, "syn", "nonexistent", None);
        assert!(
            results.is_empty(),
            "nonexistent subdir should return empty results"
        );
    }

    #[test]
    fn skips_hidden_files() {
        let (dir, oikos) = setup_oikos();
        mkfile(dir.path(), "shared/tools/.hidden.md");
        mkfile(dir.path(), "shared/tools/visible.md");

        let results = discover(&oikos, "syn", "tools", Some("md"));
        assert_eq!(results.len(), 1, "hidden files should be excluded");
        assert_eq!(
            results[0].name, "visible.md",
            "only visible file should be returned"
        );
    }

    #[test]
    fn resolve_most_specific() {
        let (dir, oikos) = setup_oikos();
        mkfile(dir.path(), "nous/syn/USER.md");
        mkfile(dir.path(), "theke/USER.md");

        let path = resolve(&oikos, "syn", "USER.md", None);
        assert!(path.is_some(), "resolve should find USER.md in nous tier");
        assert!(
            path.unwrap().to_string_lossy().contains("nous/syn"),
            "resolve should prefer nous tier"
        );
    }

    #[test]
    fn resolve_falls_to_theke() {
        let (dir, oikos) = setup_oikos();
        mkfile(dir.path(), "theke/USER.md");

        let path = resolve(&oikos, "syn", "USER.md", None);
        assert!(path.is_some(), "resolve should find USER.md in theke tier");
        assert!(
            path.unwrap().to_string_lossy().contains("theke"),
            "resolve should fall back to theke"
        );
    }

    #[test]
    fn resolve_returns_none_for_missing() {
        let (_dir, oikos) = setup_oikos();
        let path = resolve(&oikos, "syn", "NONEXISTENT.md", None);
        assert!(
            path.is_none(),
            "resolve should return None for missing file"
        );
    }

    #[test]
    fn resolve_all_returns_all_tiers() {
        let (dir, oikos) = setup_oikos();
        mkfile(dir.path(), "nous/syn/config.yaml");
        mkfile(dir.path(), "shared/config.yaml");
        mkfile(dir.path(), "theke/config.yaml");

        let results = resolve_all(&oikos, "syn", "config.yaml", None);
        assert_eq!(
            results.len(),
            3,
            "resolve_all should find file in all three tiers"
        );
        assert_eq!(
            results[0].tier,
            Tier::Nous,
            "first result should be nous tier"
        );
        assert_eq!(
            results[1].tier,
            Tier::Shared,
            "second result should be shared tier"
        );
        assert_eq!(
            results[2].tier,
            Tier::Theke,
            "third result should be theke tier"
        );
    }

    #[test]
    fn different_agents_isolated() {
        let (dir, oikos) = setup_oikos();
        mkfile(dir.path(), "nous/syn/tools/syn-only.md");
        mkfile(dir.path(), "nous/demiurge/tools/demi-only.md");
        mkfile(dir.path(), "shared/tools/common.md");

        let syn = discover(&oikos, "syn", "tools", Some("md"));
        let demi = discover(&oikos, "demiurge", "tools", Some("md"));

        let syn_names: Vec<&str> = syn.iter().map(|r| r.name.as_str()).collect();
        let demi_names: Vec<&str> = demi.iter().map(|r| r.name.as_str()).collect();

        assert!(
            syn_names.contains(&"syn-only.md"),
            "syn should see its own file"
        );
        assert!(
            !syn_names.contains(&"demi-only.md"),
            "syn should not see demiurge file"
        );
        assert!(
            demi_names.contains(&"demi-only.md"),
            "demiurge should see its own file"
        );
        assert!(
            !demi_names.contains(&"syn-only.md"),
            "demiurge should not see syn file"
        );
        assert!(
            syn_names.contains(&"common.md"),
            "syn should see shared file"
        );
        assert!(
            demi_names.contains(&"common.md"),
            "demiurge should see shared file"
        );
    }

    #[test]
    fn discover_no_extension_filter() {
        let (dir, oikos) = setup_oikos();
        mkfile(dir.path(), "shared/tools/tool.md");
        mkfile(dir.path(), "shared/tools/tool.yaml");
        mkfile(dir.path(), "shared/tools/readme.txt");

        let results = discover(&oikos, "syn", "tools", None);
        assert_eq!(
            results.len(),
            3,
            "no extension filter should return all files"
        );
    }

    #[test]
    fn resolve_with_subdir() {
        let (dir, oikos) = setup_oikos();
        mkfile(dir.path(), "nous/syn/hooks/pre-turn.sh");

        let found = resolve(&oikos, "syn", "pre-turn.sh", Some("hooks"));
        assert!(found.is_some(), "resolve with subdir should find the file");
        assert!(
            found
                .unwrap()
                .to_string_lossy()
                .contains("hooks/pre-turn.sh"),
            "resolved path should contain subdir and filename"
        );
    }

    #[test]
    fn resolve_all_partial_tiers() {
        let (dir, oikos) = setup_oikos();
        mkfile(dir.path(), "nous/syn/config.yaml");
        mkfile(dir.path(), "theke/config.yaml");

        let results = resolve_all(&oikos, "syn", "config.yaml", None);
        assert_eq!(
            results.len(),
            2,
            "resolve_all should find file in two tiers"
        );
        assert_eq!(
            results[0].tier,
            Tier::Nous,
            "first result should be nous tier"
        );
        assert_eq!(
            results[1].tier,
            Tier::Theke,
            "second result should be theke tier"
        );
    }

    #[test]
    fn tier_display() {
        assert_eq!(
            Tier::Nous.to_string(),
            "nous",
            "Nous tier display should be 'nous'"
        );
        assert_eq!(
            Tier::Shared.to_string(),
            "shared",
            "Shared tier display should be 'shared'"
        );
        assert_eq!(
            Tier::Theke.to_string(),
            "theke",
            "Theke tier display should be 'theke'"
        );
    }

    #[test]
    fn resolve_all_empty_when_no_match() {
        let (_dir, oikos) = setup_oikos();
        let results = resolve_all(&oikos, "syn", "nonexistent.md", None);
        assert!(
            results.is_empty(),
            "resolve_all should return empty for missing file"
        );
    }

    // ── *_with variants (FileSystem trait) ───────────────────────────────

    fn in_memory_oikos() -> Oikos {
        Oikos::from_root("/instance")
    }

    #[test]
    fn discover_with_finds_files_across_tiers() {
        use aletheia_koina::system::TestSystem;

        let mut fs = TestSystem::new();
        fs.add_file("/instance/nous/syn/tools/agent.md", b"a");
        fs.add_file("/instance/shared/tools/shared.md", b"b");
        fs.add_file("/instance/theke/tools/theke.md", b"c");

        let oikos = in_memory_oikos();
        let results = discover_with(&fs, &oikos, "syn", "tools", Some("md"));
        assert_eq!(
            results.len(),
            3,
            "in-memory discover should find all three tiers"
        );

        let names: Vec<&str> = results.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"agent.md"), "should include nous-tier file");
        assert!(
            names.contains(&"shared.md"),
            "should include shared-tier file"
        );
        assert!(
            names.contains(&"theke.md"),
            "should include theke-tier file"
        );
    }

    #[test]
    fn discover_with_most_specific_wins() {
        use aletheia_koina::system::TestSystem;

        let mut fs = TestSystem::new();
        fs.add_file("/instance/nous/syn/tools/common.md", b"nous");
        fs.add_file("/instance/shared/tools/common.md", b"shared");

        let oikos = in_memory_oikos();
        let results = discover_with(&fs, &oikos, "syn", "tools", Some("md"));
        assert_eq!(
            results.len(),
            1,
            "duplicate name should resolve to one entry"
        );
        assert_eq!(
            results[0].tier,
            Tier::Nous,
            "nous tier should win in-memory"
        );
    }

    #[test]
    fn discover_with_skips_hidden_files() {
        use aletheia_koina::system::TestSystem;

        let mut fs = TestSystem::new();
        fs.add_file("/instance/shared/tools/.hidden.md", b"hidden");
        fs.add_file("/instance/shared/tools/visible.md", b"visible");

        let oikos = in_memory_oikos();
        let results = discover_with(&fs, &oikos, "syn", "tools", Some("md"));
        assert_eq!(
            results.len(),
            1,
            "hidden files should be excluded in-memory"
        );
        assert_eq!(
            results[0].name, "visible.md",
            "only visible file should be returned"
        );
    }

    #[test]
    fn discover_with_filters_by_extension() {
        use aletheia_koina::system::TestSystem;

        let mut fs = TestSystem::new();
        fs.add_file("/instance/shared/tools/tool.md", b"md");
        fs.add_file("/instance/shared/tools/tool.yaml", b"yaml");

        let oikos = in_memory_oikos();
        let md = discover_with(&fs, &oikos, "syn", "tools", Some("md"));
        let yaml = discover_with(&fs, &oikos, "syn", "tools", Some("yaml"));
        assert_eq!(md.len(), 1, "md filter should match one file");
        assert_eq!(md[0].name, "tool.md", "md filter should return .md file");
        assert_eq!(yaml.len(), 1, "yaml filter should match one file");
        assert_eq!(
            yaml[0].name, "tool.yaml",
            "yaml filter should return .yaml file"
        );
    }

    #[test]
    fn resolve_with_returns_most_specific() {
        use aletheia_koina::system::TestSystem;

        let mut fs = TestSystem::new();
        fs.add_file("/instance/nous/syn/USER.md", b"nous");
        fs.add_file("/instance/theke/USER.md", b"theke");

        let oikos = in_memory_oikos();
        let found = resolve_with(&fs, &oikos, "syn", "USER.md", None);
        assert!(found.is_some(), "resolve_with should find USER.md");
        let path = found.unwrap();
        assert!(
            path.to_string_lossy().contains("nous/syn"),
            "resolve_with should prefer nous tier"
        );
    }

    #[test]
    fn resolve_with_falls_back_to_theke() {
        use aletheia_koina::system::TestSystem;

        let mut fs = TestSystem::new();
        fs.add_file("/instance/theke/SYSTEM.md", b"theke");

        let oikos = in_memory_oikos();
        let found = resolve_with(&fs, &oikos, "syn", "SYSTEM.md", None);
        assert!(
            found.is_some(),
            "resolve_with should find SYSTEM.md in theke"
        );
        assert!(
            found.unwrap().to_string_lossy().contains("theke"),
            "resolve_with should fall back to theke"
        );
    }

    #[test]
    fn resolve_with_returns_none_when_absent() {
        use aletheia_koina::system::TestSystem;

        let fs = TestSystem::new();
        let oikos = in_memory_oikos();
        assert!(
            resolve_with(&fs, &oikos, "syn", "MISSING.md", None).is_none(),
            "absent file should return None"
        );
    }

    #[test]
    fn resolve_all_with_returns_all_tiers() {
        use aletheia_koina::system::TestSystem;

        let mut fs = TestSystem::new();
        fs.add_file("/instance/nous/syn/config.toml", b"nous");
        fs.add_file("/instance/shared/config.toml", b"shared");
        fs.add_file("/instance/theke/config.toml", b"theke");

        let oikos = in_memory_oikos();
        let results = resolve_all_with(&fs, &oikos, "syn", "config.toml", None);
        assert_eq!(
            results.len(),
            3,
            "resolve_all_with should find all three tiers"
        );
        assert_eq!(results[0].tier, Tier::Nous, "first result should be nous");
        assert_eq!(
            results[1].tier,
            Tier::Shared,
            "second result should be shared"
        );
        assert_eq!(results[2].tier, Tier::Theke, "third result should be theke");
    }

    #[test]
    fn resolve_all_with_empty_when_no_match() {
        use aletheia_koina::system::TestSystem;

        let fs = TestSystem::new();
        let oikos = in_memory_oikos();
        let results = resolve_all_with(&fs, &oikos, "syn", "none.md", None);
        assert!(
            results.is_empty(),
            "resolve_all_with should return empty for missing file"
        );
    }
}
