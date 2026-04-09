//! Shared fixtures for taxis integration test binaries.
//!
//! WHY: placed under `tests/common/mod.rs` so Cargo does NOT compile it as
//! its own test binary; it is only reachable via `mod common;` from the
//! real top-level test files in `tests/`.

#![expect(clippy::expect_used, reason = "test fixtures use expect on setup failures")]
#![expect(
    clippy::disallowed_methods,
    reason = "fixtures need std::fs::write to seed real file content"
)]
// WHY: #[allow] rather than #[expect] because each test binary uses a different
// subset of these helpers (e.g. public_api_oikos does not call write_toml).
// #[expect] would be unfulfilled in binaries that happen to use every helper.
#![allow(dead_code)]

use std::path::Path;

use tempfile::TempDir;

/// Create a temp instance root with the minimal `config/`, `data/`, and
/// `nous/` subdirectories that `Oikos::validate` requires.
pub fn make_valid_instance() -> TempDir {
    let dir = tempfile::tempdir().expect("create temp dir");
    std::fs::create_dir_all(dir.path().join("config")).expect("create config dir");
    std::fs::create_dir_all(dir.path().join("data")).expect("create data dir");
    std::fs::create_dir_all(dir.path().join("nous")).expect("create nous dir");
    dir
}

/// Write a config file into `<instance>/config/aletheia.toml`.
pub fn write_toml(instance: &Path, body: &str) {
    let cfg_dir = instance.join("config");
    std::fs::create_dir_all(&cfg_dir).expect("create config dir");
    std::fs::write(cfg_dir.join("aletheia.toml"), body).expect("write aletheia.toml");
}
