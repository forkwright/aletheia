//! Compile-time build identity: git commit SHA, working-tree dirty flag,
//! build timestamp, and crate version.
//!
//! WHY (#7208): the served binary previously had no build-time mechanism
//! populating build provenance at all — `pylon`'s health handler read
//! `option_env!("GIT_SHA")`, but nothing ever set that variable, so every
//! response silently fell back to the literal `"unknown"`. `koina` is "the
//! common foundation that every crate depends on" (see the crate root
//! doc), so this module is the single place that reads
//! [`../build.rs`](../build.rs)'s `cargo:rustc-env` output — every
//! consumer (`aletheia --version`, `/api/v1/system/health`, the startup
//! log line) reports the same values, sourced once.

/// Full 40-character git HEAD commit SHA at build time, or the explicit
/// literal `"unknown"` when the build ran outside a git checkout or `git`
/// was unavailable. Never a fabricated SHA — see `build.rs::git_head_sha`.
pub const GIT_SHA: &str = env!("KOINA_BUILD_GIT_SHA");

/// `"true"` when the working tree carried pending changes at build time
/// (per `git status --porcelain`), `"false"` otherwise. Kept as the raw
/// build-script string (rather than parsed at const-eval time) so this
/// stays a plain `env!` read all the way through, like [`GIT_SHA`]; use
/// [`git_dirty`] for a `bool`.
pub const GIT_DIRTY_STR: &str = env!("KOINA_BUILD_GIT_DIRTY");

/// Unix seconds (UTC) when this crate was compiled. `0` only if the
/// build-time clock read itself failed (system clock before the Unix
/// epoch) — not reachable on any real build host.
pub const BUILD_TIMESTAMP_UNIX: i64 = parse_i64_or_zero(env!("KOINA_BUILD_TIMESTAMP_UNIX"));

/// Crate version from `Cargo.toml` — the single workspace version every
/// aletheia crate shares.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

const fn parse_i64_or_zero(s: &str) -> i64 {
    match i64::from_str_radix(s, 10) {
        Ok(v) => v,
        Err(_) => 0,
    }
}

/// [`GIT_DIRTY_STR`] as a `bool`.
#[must_use]
pub fn git_dirty() -> bool {
    GIT_DIRTY_STR == "true"
}

/// RFC 3339 UTC rendering of [`BUILD_TIMESTAMP_UNIX`]. Falls back to the
/// literal `"unknown"` in the unreachable case that the build-time clock
/// read failed — this must never panic, since it feeds the health
/// endpoint and the startup log line.
#[must_use]
pub fn build_timestamp() -> String {
    if BUILD_TIMESTAMP_UNIX == 0 {
        return "unknown".to_owned();
    }
    jiff::Timestamp::from_second(BUILD_TIMESTAMP_UNIX)
        .map_or_else(|_| "unknown".to_owned(), |ts| ts.to_string())
}

/// One-line human-readable build identity, e.g.
/// `"0.45.0 (326dd9ab3..., dirty, built 2026-09-06T18:03:11Z)"`.
/// Used for `aletheia --version` and the startup log line.
#[must_use]
pub fn version_line() -> String {
    let dirty_suffix = if git_dirty() { ", dirty" } else { "" };
    format!(
        "{CRATE_VERSION} ({GIT_SHA}{dirty_suffix}, built {})",
        build_timestamp()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #7208: building this crate happens inside a git checkout (this repo,
    /// as it is building right now), so the embedded identity must be
    /// genuinely resolved, not the "unknown"/empty fallback.
    #[test]
    fn identity_fields_are_populated_in_a_git_checkout() {
        assert!(!GIT_SHA.is_empty(), "GIT_SHA must not be empty");
        assert_ne!(
            GIT_SHA, "unknown",
            "building inside this git checkout must resolve a real HEAD SHA"
        );
        assert_eq!(
            GIT_SHA.len(),
            40,
            "git rev-parse HEAD returns a full 40-character SHA, got {GIT_SHA:?}"
        );
        assert!(
            GIT_SHA.bytes().all(|b| b.is_ascii_hexdigit()),
            "GIT_SHA must be hex, got {GIT_SHA:?}"
        );

        assert!(
            matches!(GIT_DIRTY_STR, "true" | "false"),
            "GIT_DIRTY_STR must be a literal \"true\" or \"false\", got {GIT_DIRTY_STR:?}"
        );

        assert_ne!(
            BUILD_TIMESTAMP_UNIX, 0,
            "BUILD_TIMESTAMP_UNIX must be a real epoch timestamp"
        );
        assert_ne!(
            build_timestamp(),
            "unknown",
            "a real BUILD_TIMESTAMP_UNIX must render to a real RFC 3339 timestamp"
        );

        assert!(!CRATE_VERSION.is_empty(), "CRATE_VERSION must not be empty");
    }

    #[test]
    fn version_line_contains_every_identity_field() {
        let line = version_line();
        assert!(line.contains(CRATE_VERSION));
        assert!(line.contains(GIT_SHA));
        assert!(line.contains("built"));
    }
}
