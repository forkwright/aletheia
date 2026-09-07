//! Compile-time validation for the embedded model seed, plus compile-time
//! build-identity embedding (git SHA, dirty flag, build timestamp).
//!
//! WHY (#5635): `MODEL_SEED` in `src/models.rs` is initialized from this file at
//! runtime via `toml::from_str`. A malformed file compiles successfully because
//! `include_str!` only checks existence, then panics on first access. Parsing it
//! here converts the production crash path into a build error.
//!
//! WHY (#7025): the schema below is not a hand-maintained copy of the runtime
//! `ModelSeed` types — it IS them, via `include!` of
//! `src/model_seed_schema.rs`, the single file both this build script and
//! `src/models.rs` splice in. A build that accepts a seed is therefore
//! guaranteed to accept the same seed at runtime, because there is only one
//! schema to accept it against.
//!
//! WHY (#7208): koina is "the common foundation that every crate depends
//! on" (see `src/lib.rs`), so it is the one place to shell out to `git` at
//! build time and embed the result — every consumer (`aletheia --version`,
//! `/api/v1/system/health`, the startup log line) reads the same
//! [`src/build_info.rs`] constants instead of each growing its own
//! `option_env!` copy that nothing ever sets (the bug this closes: the
//! served binary had no build-time mechanism populating `GIT_SHA` at all).

use std::env;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/model_seed_schema.rs"
));

fn main() -> io::Result<()> {
    let manifest_dir = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR")
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?,
    );
    let seed_path = manifest_dir.join("data/model-seed.toml");
    let schema_path = manifest_dir.join("src/model_seed_schema.rs");

    println!("cargo:rerun-if-changed={}", seed_path.display());
    println!("cargo:rerun-if-changed={}", schema_path.display());

    let seed_text = std::fs::read_to_string(&seed_path)?;
    toml::from_str::<ModelSeed>(&seed_text)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{e}")))?;

    emit_build_identity(&manifest_dir);

    Ok(())
}

/// Embed the git HEAD SHA, working-tree dirty flag, and build timestamp as
/// `cargo:rustc-env` values that [`src/build_info.rs`] reads back with
/// `env!`. Never fabricates a SHA: a build outside a git checkout (no
/// `.git`, no `git` binary, or any other failure) gets the explicit literal
/// `"unknown"`.
fn emit_build_identity(manifest_dir: &Path) {
    let workspace_root = manifest_dir.join("../..");

    let sha = git_head_sha(&workspace_root).unwrap_or_else(|| "unknown".to_owned());
    let dirty = git_tree_is_dirty(&workspace_root);
    let build_time_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());

    println!("cargo:rustc-env=KOINA_BUILD_GIT_SHA={sha}");
    println!("cargo:rustc-env=KOINA_BUILD_GIT_DIRTY={dirty}");
    println!("cargo:rustc-env=KOINA_BUILD_TIMESTAMP_UNIX={build_time_unix}");

    // WHY: rebuild when HEAD moves (checkout/rebase/commit-on-branch, via
    // the reflog), a detached HEAD is repointed directly, or the index
    // changes (staging, or a commit's post-write stat refresh), so a stale
    // embedded SHA/dirty-flag never survives a rebuild. Resolved through
    // `git rev-parse --git-path`, not a hardcoded `.git/<name>` join:
    // `.git` is a *file* (a `gitdir:` pointer) in a git worktree checkout
    // — the layout every PR in this fleet is built from — so the real
    // HEAD/index/logs live under the linked common dir, not under this
    // checkout's `.git/`. `.git/HEAD` alone is not enough: it changes on
    // checkout/detach but *not* on an ordinary commit to the already
    // checked-out branch, whereas the reflog (`logs/HEAD`) is appended on
    // every one of those operations.
    for git_relative in ["HEAD", "index", "logs/HEAD"] {
        if let Some(path) = git_watch_path(&workspace_root, git_relative) {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
}

/// Resolve `git_relative` (e.g. `"HEAD"`, `"logs/HEAD"`) to the real
/// filesystem path cargo should watch, via `git rev-parse --git-path`
/// rather than assuming `<workspace_root>/.git/<git_relative>` — the latter
/// resolves to a nonexistent path in a git worktree checkout, where `.git`
/// is a `gitdir:` pointer file rather than the actual git directory.
/// Returns `None` (skip the watch) if `git` is unavailable; there is
/// nothing accurate to watch in that case, and `sha`/`dirty` already fall
/// back to `"unknown"`/not-fabricated above.
fn git_watch_path(workspace_root: &Path, git_relative: &str) -> Option<PathBuf> {
    let out = git_command(workspace_root)
        .args(["rev-parse", "--git-path", git_relative])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if raw.is_empty() {
        return None;
    }
    let path = PathBuf::from(raw);
    // WHY: `--git-path` prints a path relative to the cwd it ran with (a
    // plain clone: `.git/HEAD`) in some layouts and an absolute path (a
    // worktree: `/repo/.git/worktrees/<name>/HEAD`) in others — join onto
    // `workspace_root` (this command's cwd) only when it's still relative.
    Some(if path.is_absolute() {
        path
    } else {
        workspace_root.join(path)
    })
}

/// Build a `git` invocation isolated from the caller's ambient git
/// environment.
///
/// WHY: `git` honors `GIT_DIR`/`GIT_WORK_TREE`/`GIT_INDEX_FILE` from the
/// process environment ahead of the process's working directory, so a
/// build script reached from inside another repo's process tree (e.g. a
/// git hook) could otherwise report that repo's state instead of this
/// workspace's.
fn git_command(workspace_root: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(workspace_root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE");
    cmd
}

fn git_head_sha(workspace_root: &Path) -> Option<String> {
    let out = git_command(workspace_root)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let sha = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if sha.is_empty() { None } else { Some(sha) }
}

/// `true` when `git status --porcelain` reports any pending changes
/// (staged, unstaged, or untracked). Defaults to `false` (not fabricated
/// "clean") only when `git` itself could not be run at all; a `git`
/// invocation that runs and succeeds is trusted as-is.
fn git_tree_is_dirty(workspace_root: &Path) -> bool {
    git_command(workspace_root)
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .and_then(|out| out.status.success().then_some(out.stdout))
        .is_some_and(|stdout| !stdout.is_empty())
}
