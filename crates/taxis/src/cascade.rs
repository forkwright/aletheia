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
    let tiers = [
        (Tier::Nous, oikos.nous_dir(nous_id).join(subdir)),
        (Tier::Shared, oikos.shared().join(subdir)),
        (Tier::Theke, oikos.theke().join(subdir)),
    ];

    let mut seen: HashMap<String, CascadeEntry> = HashMap::new();

    for (tier, dir) in &tiers {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };

        for entry in entries.flatten() {
            let path = entry.path();

            if !path.is_file() {
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
#[must_use]
pub fn resolve(
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
        if candidate.exists() {
            debug!(?candidate, filename, "cascade resolved");
            return Some(candidate);
        }
    }

    None
}

/// Resolve all instances of a named file across all tiers.
///
/// Returns matches ordered most-specific first. Useful for config deep-merge
/// where all tiers contribute.
#[must_use]
pub fn resolve_all(
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
        .filter(|(_, path)| path.exists())
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
    use super::*;
    use std::fs;
    use std::path::Path;

    fn setup_oikos() -> (tempfile::TempDir, Oikos) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let oikos = Oikos::from_root(dir.path());
        (dir, oikos)
    }

    fn mkfile(base: &Path, rel: &str) {
        let path = base.join(rel);
        fs::create_dir_all(path.parent().unwrap()).expect("create parent dirs");
        fs::write(&path, format!("content of {rel}")).expect("write file");
    }

    #[test]
    fn discovers_from_all_tiers() {
        let (dir, oikos) = setup_oikos();
        mkfile(dir.path(), "nous/syn/tools/agent-only.md");
        mkfile(dir.path(), "shared/tools/shared-tool.md");
        mkfile(dir.path(), "theke/tools/theke-tool.md");

        let results = discover(&oikos, "syn", "tools", Some("md"));
        assert_eq!(results.len(), 3);

        let names: Vec<&str> = results.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"agent-only.md"));
        assert!(names.contains(&"shared-tool.md"));
        assert!(names.contains(&"theke-tool.md"));
    }

    #[test]
    fn most_specific_wins() {
        let (dir, oikos) = setup_oikos();
        mkfile(dir.path(), "nous/syn/tools/override.md");
        mkfile(dir.path(), "shared/tools/override.md");
        mkfile(dir.path(), "theke/tools/override.md");

        let results = discover(&oikos, "syn", "tools", Some("md"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].tier, Tier::Nous);
    }

    #[test]
    fn shared_overrides_theke() {
        let (dir, oikos) = setup_oikos();
        mkfile(dir.path(), "shared/hooks/common.md");
        mkfile(dir.path(), "theke/hooks/common.md");

        let results = discover(&oikos, "syn", "hooks", Some("md"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].tier, Tier::Shared);
    }

    #[test]
    fn extension_filter() {
        let (dir, oikos) = setup_oikos();
        mkfile(dir.path(), "shared/tools/tool.md");
        mkfile(dir.path(), "shared/tools/tool.yaml");

        let md = discover(&oikos, "syn", "tools", Some("md"));
        assert_eq!(md.len(), 1);
        assert_eq!(md[0].name, "tool.md");

        let yaml = discover(&oikos, "syn", "tools", Some("yaml"));
        assert_eq!(yaml.len(), 1);
        assert_eq!(yaml[0].name, "tool.yaml");
    }

    #[test]
    fn missing_dirs_return_empty() {
        let (_dir, oikos) = setup_oikos();
        let results = discover(&oikos, "syn", "nonexistent", None);
        assert!(results.is_empty());
    }

    #[test]
    fn skips_hidden_files() {
        let (dir, oikos) = setup_oikos();
        mkfile(dir.path(), "shared/tools/.hidden.md");
        mkfile(dir.path(), "shared/tools/visible.md");

        let results = discover(&oikos, "syn", "tools", Some("md"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "visible.md");
    }

    #[test]
    fn resolve_most_specific() {
        let (dir, oikos) = setup_oikos();
        mkfile(dir.path(), "nous/syn/USER.md");
        mkfile(dir.path(), "theke/USER.md");

        let path = resolve(&oikos, "syn", "USER.md", None);
        assert!(path.is_some());
        assert!(path.unwrap().to_string_lossy().contains("nous/syn"));
    }

    #[test]
    fn resolve_falls_to_theke() {
        let (dir, oikos) = setup_oikos();
        mkfile(dir.path(), "theke/USER.md");

        let path = resolve(&oikos, "syn", "USER.md", None);
        assert!(path.is_some());
        assert!(path.unwrap().to_string_lossy().contains("theke"));
    }

    #[test]
    fn resolve_returns_none_for_missing() {
        let (_dir, oikos) = setup_oikos();
        let path = resolve(&oikos, "syn", "NONEXISTENT.md", None);
        assert!(path.is_none());
    }

    #[test]
    fn resolve_all_returns_all_tiers() {
        let (dir, oikos) = setup_oikos();
        mkfile(dir.path(), "nous/syn/config.yaml");
        mkfile(dir.path(), "shared/config.yaml");
        mkfile(dir.path(), "theke/config.yaml");

        let results = resolve_all(&oikos, "syn", "config.yaml", None);
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].tier, Tier::Nous);
        assert_eq!(results[1].tier, Tier::Shared);
        assert_eq!(results[2].tier, Tier::Theke);
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

        assert!(syn_names.contains(&"syn-only.md"));
        assert!(!syn_names.contains(&"demi-only.md"));
        assert!(demi_names.contains(&"demi-only.md"));
        assert!(!demi_names.contains(&"syn-only.md"));
        assert!(syn_names.contains(&"common.md"));
        assert!(demi_names.contains(&"common.md"));
    }

    #[test]
    fn discover_no_extension_filter() {
        let (dir, oikos) = setup_oikos();
        mkfile(dir.path(), "shared/tools/tool.md");
        mkfile(dir.path(), "shared/tools/tool.yaml");
        mkfile(dir.path(), "shared/tools/readme.txt");

        let results = discover(&oikos, "syn", "tools", None);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn resolve_with_subdir() {
        let (dir, oikos) = setup_oikos();
        mkfile(dir.path(), "nous/syn/hooks/pre-turn.sh");

        let found = resolve(&oikos, "syn", "pre-turn.sh", Some("hooks"));
        assert!(found.is_some());
        assert!(
            found
                .unwrap()
                .to_string_lossy()
                .contains("hooks/pre-turn.sh")
        );
    }

    #[test]
    fn resolve_all_partial_tiers() {
        let (dir, oikos) = setup_oikos();
        mkfile(dir.path(), "nous/syn/config.yaml");
        mkfile(dir.path(), "theke/config.yaml");

        let results = resolve_all(&oikos, "syn", "config.yaml", None);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].tier, Tier::Nous);
        assert_eq!(results[1].tier, Tier::Theke);
    }

    #[test]
    fn tier_display() {
        assert_eq!(Tier::Nous.to_string(), "nous");
        assert_eq!(Tier::Shared.to_string(), "shared");
        assert_eq!(Tier::Theke.to_string(), "theke");
    }

    #[test]
    fn resolve_all_empty_when_no_match() {
        let (_dir, oikos) = setup_oikos();
        let results = resolve_all(&oikos, "syn", "nonexistent.md", None);
        assert!(results.is_empty());
    }
}
