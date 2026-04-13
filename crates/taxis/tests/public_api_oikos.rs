//! Integration tests for taxis's Oikos (instance path resolver), cascade
//! discovery, startup validation, and preflight.
//!
//! Part of aletheia#2814. Sibling binary to `tests/public_api.rs`.

#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::disallowed_methods,
    reason = "integration tests need std::fs::write to stage real file fixtures"
)]
#![expect(
    clippy::indexing_slicing,
    reason = "test assertions index after asserting length or presence"
)]

mod common;

use std::path::{Path, PathBuf};

use serde_json::json;

use taxis::cascade::{self, Tier};
use taxis::config::{AletheiaConfig, NousDefinition};
use taxis::error::Error;
use taxis::oikos::Oikos;
use taxis::preflight::check_preconditions;
use taxis::validate::{validate_section, validate_startup};

use common::make_valid_instance;

// ─── Oikos: path accessors ──────────────────────────────────────────────

#[test]
fn oikos_from_root_preserves_nonexistent_path_verbatim() {
    // WHY: Oikos::from_root canonicalizes when the path exists on disk, but
    // must store the raw path when it does not (init scenario).
    let oikos = Oikos::from_root("/tmp/aletheia-taxis-test-nonexistent-xyz-1234");
    assert_eq!(
        oikos.root(),
        Path::new("/tmp/aletheia-taxis-test-nonexistent-xyz-1234")
    );
}

#[test]
fn oikos_nous_dir_includes_agent_id_segment() {
    let oikos = Oikos::from_root("/srv/instance");
    assert_eq!(
        oikos.nous_dir("syn"),
        PathBuf::from("/srv/instance/nous/syn")
    );
}

#[test]
fn oikos_nous_file_joins_agent_dir_and_filename() {
    let oikos = Oikos::from_root("/srv/instance");
    assert_eq!(
        oikos.nous_file("phrouros", "SOUL.md"),
        PathBuf::from("/srv/instance/nous/phrouros/SOUL.md")
    );
}

#[test]
fn oikos_data_subpaths_are_under_data_directory() {
    let oikos = Oikos::from_root("/srv/instance");
    assert_eq!(oikos.data(), PathBuf::from("/srv/instance/data"));
    assert_eq!(
        oikos.sessions_db(),
        PathBuf::from("/srv/instance/data/sessions.db")
    );
    assert_eq!(
        oikos.knowledge_db(),
        PathBuf::from("/srv/instance/data/knowledge.fjall")
    );
    assert_eq!(oikos.backups(), PathBuf::from("/srv/instance/data/backups"));
    assert_eq!(oikos.archive(), PathBuf::from("/srv/instance/data/archive"));
}

#[test]
fn oikos_log_subpaths_are_under_logs_directory() {
    let oikos = Oikos::from_root("/srv/instance");
    assert_eq!(oikos.logs(), PathBuf::from("/srv/instance/logs"));
    assert_eq!(
        oikos.traces(),
        PathBuf::from("/srv/instance/logs/traces")
    );
    assert_eq!(
        oikos.trace_archive(),
        PathBuf::from("/srv/instance/logs/traces/archive")
    );
}

#[test]
fn oikos_config_and_credentials_paths_match_standard_layout() {
    let oikos = Oikos::from_root("/srv/instance");
    assert_eq!(oikos.config(), PathBuf::from("/srv/instance/config"));
    assert_eq!(
        oikos.credentials(),
        PathBuf::from("/srv/instance/config/credentials")
    );
}

#[test]
fn oikos_theke_and_shared_paths_are_under_root() {
    let oikos = Oikos::from_root("/srv/instance");
    assert_eq!(oikos.theke(), PathBuf::from("/srv/instance/theke"));
    assert_eq!(oikos.shared(), PathBuf::from("/srv/instance/shared"));
}

// ─── Oikos: validate() ─────────────────────────────────────────────────

#[test]
fn oikos_validate_succeeds_for_well_formed_instance() {
    let dir = make_valid_instance();
    assert!(Oikos::from_root(dir.path()).validate().is_ok());
}

#[test]
fn oikos_validate_fails_when_root_directory_missing() {
    let oikos = Oikos::from_root("/tmp/aletheia-taxis-missing-root-xyz-8675309");
    let err = oikos.validate().unwrap_err();
    // WHY: match the typed error variant, not string content -- string
    // assertions couple tests to Display formatting.
    assert!(
        matches!(err, Error::InstanceRootNotFound { .. }),
        "expected InstanceRootNotFound, got {err:?}"
    );
}

#[test]
fn oikos_validate_fails_when_config_subdirectory_missing() {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::create_dir_all(dir.path().join("data")).expect("mk data");

    let err = Oikos::from_root(dir.path()).validate().unwrap_err();
    assert!(
        matches!(err, Error::RequiredDirMissing { .. }),
        "expected RequiredDirMissing, got {err:?}"
    );
}

#[test]
fn oikos_validate_workspace_path_accepts_existing_relative_dir() {
    let dir = make_valid_instance();
    let oikos = Oikos::from_root(dir.path());
    assert!(oikos.validate_workspace_path("nous").is_ok());
}

#[test]
fn oikos_validate_workspace_path_rejects_missing_directory() {
    let dir = make_valid_instance();
    let oikos = Oikos::from_root(dir.path());
    let err = oikos
        .validate_workspace_path("nous/nonexistent-agent")
        .unwrap_err();
    assert!(
        matches!(err, Error::WorkspacePathInvalid { .. }),
        "expected WorkspacePathInvalid, got {err:?}"
    );
}

// ─── validate_startup ──────────────────────────────────────────────────

#[test]
fn validate_startup_rejects_config_with_empty_agent_list() {
    let dir = make_valid_instance();
    let oikos = Oikos::from_root(dir.path());
    let err = validate_startup(&AletheiaConfig::default(), &oikos).unwrap_err();
    assert!(
        err.errors.iter().any(|e| e.contains("empty")),
        "error should mention empty agent list: {err:?}"
    );
}

#[test]
fn validate_startup_rejects_agent_with_nonexistent_workspace() {
    let dir = make_valid_instance();
    let oikos = Oikos::from_root(dir.path());
    let mut config = AletheiaConfig::default();
    config.agents.list.push(NousDefinition {
        id: "alice".to_owned(),
        name: None,
        model: None,
        workspace: "nous/alice-missing".to_owned(),
        thinking_enabled: None,
        agency: None,
        allowed_roots: Vec::new(),
        domains: Vec::new(),
        default: false,
        recall: None,
        behavior: None,
    });

    let err = validate_startup(&config, &oikos).unwrap_err();
    assert!(err.errors.iter().any(|e| e.contains("alice")));
}

#[test]
fn validate_startup_accepts_agent_with_real_workspace_directory() {
    let dir = make_valid_instance();
    std::fs::create_dir_all(dir.path().join("nous").join("bob")).expect("mk bob workspace");

    let oikos = Oikos::from_root(dir.path());
    let mut config = AletheiaConfig::default();
    config.agents.list.push(NousDefinition {
        id: "bob".to_owned(),
        name: None,
        model: None,
        workspace: "nous/bob".to_owned(),
        thinking_enabled: None,
        agency: None,
        allowed_roots: Vec::new(),
        domains: Vec::new(),
        default: false,
        recall: None,
        behavior: None,
    });
    assert!(validate_startup(&config, &oikos).is_ok());
}

// ─── Section validation (validate_section) ────────────────────────────

#[test]
fn validate_section_rejects_zero_gateway_port() {
    assert!(validate_section("gateway", &json!({ "port": 0 })).is_err());
}

#[test]
fn validate_section_rejects_out_of_range_gateway_port() {
    assert!(validate_section("gateway", &json!({ "port": 70_000 })).is_err());
}

#[test]
fn validate_section_accepts_valid_gateway_port() {
    assert!(validate_section("gateway", &json!({ "port": 8080 })).is_ok());
}

#[test]
fn validate_section_rejects_invalid_credential_source() {
    assert!(validate_section("credential", &json!({ "source": "magic" })).is_err());
}

#[test]
fn validate_section_rejects_unknown_section_name() {
    assert!(validate_section("nonexistent-section", &json!({})).is_err());
}

// ─── Cascade: three-tier file discovery ────────────────────────────────

/// Build a minimal instance with content pre-populated in each tier.
fn seed_cascade_tiers() -> (tempfile::TempDir, Oikos) {
    let dir = tempfile::tempdir().expect("temp dir");
    let nous = dir.path().join("nous").join("syn").join("tools");
    let shared = dir.path().join("shared").join("tools");
    let theke = dir.path().join("theke").join("tools");
    std::fs::create_dir_all(&nous).expect("mk nous/syn/tools");
    std::fs::create_dir_all(&shared).expect("mk shared/tools");
    std::fs::create_dir_all(&theke).expect("mk theke/tools");

    std::fs::write(nous.join("nous-only.md"), "nous").expect("write nous file");
    std::fs::write(shared.join("shared-only.md"), "shared").expect("write shared file");
    std::fs::write(theke.join("theke-only.md"), "theke").expect("write theke file");
    // collision: nous tier must win
    std::fs::write(nous.join("shadowed.md"), "nous-wins").expect("write nous shadowed");
    std::fs::write(shared.join("shadowed.md"), "shared-loses").expect("write shared shadowed");

    let oikos = Oikos::from_root(dir.path());
    (dir, oikos)
}

#[test]
fn cascade_discover_returns_files_from_all_three_tiers() {
    let (_dir, oikos) = seed_cascade_tiers();
    let entries = cascade::discover(&oikos, "syn", "tools", Some("md"));

    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"nous-only.md"), "missing nous: {names:?}");
    assert!(names.contains(&"shared-only.md"), "missing shared: {names:?}");
    assert!(names.contains(&"theke-only.md"), "missing theke: {names:?}");
}

#[test]
fn cascade_discover_nous_tier_wins_on_name_collision() {
    let (_dir, oikos) = seed_cascade_tiers();
    let entries = cascade::discover(&oikos, "syn", "tools", Some("md"));

    let shadowed = entries
        .iter()
        .find(|e| e.name == "shadowed.md")
        .expect("shadowed entry present");
    // INVARIANT: most specific tier wins when a filename appears in multiple tiers.
    assert_eq!(shadowed.tier, Tier::Nous);
}

#[test]
fn cascade_resolve_returns_most_specific_tier_path() {
    let (_dir, oikos) = seed_cascade_tiers();
    let path = cascade::resolve(&oikos, "syn", "shadowed.md", Some("tools"))
        .expect("shadowed should resolve");
    assert!(
        path.to_string_lossy().contains("nous"),
        "resolved path should be under nous/: {}",
        path.display()
    );
}

#[test]
fn cascade_resolve_returns_none_for_missing_file() {
    let (_dir, oikos) = seed_cascade_tiers();
    assert!(cascade::resolve(&oikos, "syn", "not-a-file.md", Some("tools")).is_none());
}

#[test]
fn cascade_resolve_all_returns_entries_most_specific_first() {
    let (_dir, oikos) = seed_cascade_tiers();
    let entries = cascade::resolve_all(&oikos, "syn", "shadowed.md", Some("tools"));
    assert_eq!(entries.len(), 2, "shadowed should resolve in 2 tiers: {entries:?}");
    assert_eq!(entries[0].tier, Tier::Nous, "first entry is most specific");
    assert_eq!(entries[1].tier, Tier::Shared, "second entry is shared");
}

// ─── Preflight smoke test ──────────────────────────────────────────────

#[test]
fn preflight_disk_and_permission_checks_pass_on_fresh_tempdir_instance() {
    // WHY: the port check may collide with a running aletheia on the host,
    // so we do not assert the whole result. We assert that disk space and
    // permission checks do not fail on a tempdir-backed instance.
    let dir = make_valid_instance();
    let oikos = Oikos::from_root(dir.path());
    let config = AletheiaConfig::default();

    if let Err(err) = check_preconditions(&config, &oikos) {
        for failure in &err.failures {
            assert!(
                !failure.contains("disk space"),
                "unexpected disk space failure on tempdir: {failure}"
            );
            assert!(
                !failure.contains("not readable"),
                "unexpected permission failure on tempdir: {failure}"
            );
            assert!(
                !failure.contains("not writable"),
                "unexpected write failure on tempdir: {failure}"
            );
        }
    }
}
