//! CI failure diagnosis and repair pipeline.
//!
//! The fix pipeline uses a two-tier strategy:
//! 1. **Mechanical fixes** -- deterministic repairs (fmt, clippy, expect removal)
//! 2. **LLM fixes** -- semantic repairs via agent session (type errors, logic bugs)
//!
//! Mechanical fixes are tried first because they're free and instant.
//! LLM fixes are expensive ($0.50-2.00) and slow (3-5 min).

use super::types::{CiFailureKind, FixKind};

/// Classify a CI failure by fixability.
///
/// WHY: Avoids dispatching expensive LLM agents for failures that are
/// deterministic or cannot benefit from reasoning.
#[must_use]
pub fn classify_failure(check_name: &str, log_excerpt: &str) -> CiFailureKind {
    // Format and clippy failures are always mechanical.
    let lower_name = check_name.to_lowercase();
    let lower_log = log_excerpt.to_lowercase();

    if lower_name.contains("fmt") || lower_name.contains("format") {
        return CiFailureKind::Mechanical;
    }
    if lower_name.contains("clippy") && !lower_log.contains("error[e") {
        return CiFailureKind::Mechanical;
    }
    // Whitespace and lockfile issues are mechanical.
    if lower_log.contains("trailing whitespace") || lower_log.contains("cargo.lock") {
        return CiFailureKind::Mechanical;
    }

    CiFailureKind::Semantic
}

/// Map a fix kind to the CI failure category it addresses.
#[must_use]
pub fn fix_kind_category(kind: &FixKind) -> CiFailureKind {
    match kind {
        FixKind::Format
        | FixKind::ClippyFix
        | FixKind::ExpectRemoval
        | FixKind::Whitespace
        | FixKind::GateTrailer
        | FixKind::LockfileRegen
        | FixKind::TrainingTakeTheirs => CiFailureKind::Mechanical,
        FixKind::LlmFix => CiFailureKind::Semantic,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // classify_failure tests
    // -----------------------------------------------------------------------

    #[test]
    fn classify_failure_fmt_is_mechanical() {
        assert_eq!(
            classify_failure("cargo-fmt", "Diff in src/lib.rs"),
            CiFailureKind::Mechanical
        );
    }

    #[test]
    fn classify_failure_format_check_is_mechanical() {
        assert_eq!(
            classify_failure("format-check", "rustfmt diff"),
            CiFailureKind::Mechanical
        );
    }

    #[test]
    fn classify_failure_clippy_warning_is_mechanical() {
        assert_eq!(
            classify_failure("clippy", "warning: unused import"),
            CiFailureKind::Mechanical
        );
    }

    #[test]
    fn classify_failure_clippy_with_error_code_is_semantic() {
        // WHY: error[E...] indicates a type error or compile error that
        // clippy cannot auto-fix -- needs LLM reasoning.
        assert_eq!(
            classify_failure("clippy", "error[E0308]: mismatched types"),
            CiFailureKind::Semantic
        );
    }

    #[test]
    fn classify_failure_whitespace_is_mechanical() {
        assert_eq!(
            classify_failure("lint", "trailing whitespace found"),
            CiFailureKind::Mechanical
        );
    }

    #[test]
    fn classify_failure_lockfile_is_mechanical() {
        assert_eq!(
            classify_failure("verify", "Cargo.lock is out of date"),
            CiFailureKind::Mechanical
        );
    }

    #[test]
    fn classify_failure_build_error_is_semantic() {
        assert_eq!(
            classify_failure("build", "cannot find value `x` in this scope"),
            CiFailureKind::Semantic
        );
    }

    #[test]
    fn classify_failure_test_failure_is_semantic() {
        assert_eq!(
            classify_failure("test", "test result: FAILED. 1 passed; 1 failed"),
            CiFailureKind::Semantic
        );
    }

    // -----------------------------------------------------------------------
    // fix_kind_category tests
    // -----------------------------------------------------------------------

    #[test]
    fn fix_kind_category_mechanical_kinds() {
        assert_eq!(
            fix_kind_category(&FixKind::Format),
            CiFailureKind::Mechanical
        );
        assert_eq!(
            fix_kind_category(&FixKind::ClippyFix),
            CiFailureKind::Mechanical
        );
        assert_eq!(
            fix_kind_category(&FixKind::ExpectRemoval),
            CiFailureKind::Mechanical
        );
        assert_eq!(
            fix_kind_category(&FixKind::Whitespace),
            CiFailureKind::Mechanical
        );
        assert_eq!(
            fix_kind_category(&FixKind::GateTrailer),
            CiFailureKind::Mechanical
        );
        assert_eq!(
            fix_kind_category(&FixKind::LockfileRegen),
            CiFailureKind::Mechanical
        );
        assert_eq!(
            fix_kind_category(&FixKind::TrainingTakeTheirs),
            CiFailureKind::Mechanical
        );
    }

    #[test]
    fn fix_kind_category_llm_is_semantic() {
        assert_eq!(fix_kind_category(&FixKind::LlmFix), CiFailureKind::Semantic);
    }
}
