//! Cross-crate tests for taxis oikos + cascade resolution.

use std::fs;
use std::path::Path;

use aletheia_taxis::cascade::{self, Tier};
use aletheia_taxis::oikos::Oikos;

fn setup() -> (tempfile::TempDir, Oikos) {
    let dir = tempfile::tempdir().unwrap();
    let oikos = Oikos::from_root(dir.path());
    (dir, oikos)
}

fn mkfile(base: &Path, rel: &str) {
    let path = base.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, format!("content of {rel}")).unwrap();
}

#[test]
fn discover_tools_across_all_tiers() {
    let (dir, oikos) = setup();
    mkfile(dir.path(), "nous/syn/tools/agent-tool.md");
    mkfile(dir.path(), "shared/tools/shared-tool.md");
    mkfile(dir.path(), "theke/tools/theke-tool.md");

    let results = cascade::discover(&oikos, "syn", "tools", Some("md"));
    assert_eq!(results.len(), 3);
}

#[test]
fn most_specific_tier_wins_on_collision() {
    let (dir, oikos) = setup();
    mkfile(dir.path(), "nous/syn/tools/override.md");
    mkfile(dir.path(), "shared/tools/override.md");
    mkfile(dir.path(), "theke/tools/override.md");

    let results = cascade::discover(&oikos, "syn", "tools", Some("md"));
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].tier, Tier::Nous);
}

#[test]
fn resolve_single_file_falls_through() {
    let (dir, oikos) = setup();
    mkfile(dir.path(), "theke/docs/readme.md");

    let path = cascade::resolve(&oikos, "syn", "readme.md", Some("docs"));
    assert!(path.is_some());
    assert!(path.unwrap().to_string_lossy().contains("theke"));
}

#[test]
fn resolve_all_returns_ordered_tiers() {
    let (dir, oikos) = setup();
    mkfile(dir.path(), "nous/syn/config.yaml");
    mkfile(dir.path(), "shared/config.yaml");
    mkfile(dir.path(), "theke/config.yaml");

    let results = cascade::resolve_all(&oikos, "syn", "config.yaml", None);
    assert_eq!(results.len(), 3);
    assert_eq!(results[0].tier, Tier::Nous);
    assert_eq!(results[1].tier, Tier::Shared);
    assert_eq!(results[2].tier, Tier::Theke);
}

#[test]
fn different_agents_isolated() {
    let (dir, oikos) = setup();
    mkfile(dir.path(), "nous/syn/tools/syn-only.md");
    mkfile(dir.path(), "nous/demiurge/tools/demi-only.md");
    mkfile(dir.path(), "shared/tools/common.md");

    let syn_results = cascade::discover(&oikos, "syn", "tools", Some("md"));
    let demi_results = cascade::discover(&oikos, "demiurge", "tools", Some("md"));

    let syn_names: Vec<&str> = syn_results.iter().map(|r| r.name.as_str()).collect();
    let demi_names: Vec<&str> = demi_results.iter().map(|r| r.name.as_str()).collect();

    assert!(syn_names.contains(&"syn-only.md"));
    assert!(!syn_names.contains(&"demi-only.md"));
    assert!(demi_names.contains(&"demi-only.md"));
    assert!(!demi_names.contains(&"syn-only.md"));
    assert!(syn_names.contains(&"common.md"));
    assert!(demi_names.contains(&"common.md"));
}

#[test]
fn resolve_returns_none_for_missing() {
    let (_dir, oikos) = setup();
    let path = cascade::resolve(&oikos, "syn", "NONEXISTENT.md", None);
    assert!(path.is_none());
}

#[test]
fn oikos_paths_match_structure() {
    let (dir, oikos) = setup();
    let root = dir.path();

    assert_eq!(oikos.root(), root);
    assert_eq!(oikos.shared(), root.join("shared"));
    assert_eq!(oikos.theke(), root.join("theke"));
    assert_eq!(oikos.nous_dir("syn"), root.join("nous").join("syn"));
    assert_eq!(oikos.sessions_db(), root.join("data").join("sessions.db"));
}
