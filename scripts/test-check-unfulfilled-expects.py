#!/usr/bin/env python3
"""Tests for check-unfulfilled-expects.py.

Two fixture classes: REAL (verbatim or faithfully-trimmed source from the
two production incidents this checker exists to catch -- system_status.rs
and fjall_store_tests_schema.rs, both fixed same-day on this branch's base)
and SYNTHETIC (constructed to isolate one behavior: nested-scope inclusion,
string/comment trigger-blindness, cfg exclusion, brace matching through a
brace-shaped string, and the reliable-subset boundary itself).

Each SYNTHETIC case that guards against a specific naive-implementation
mistake says so in a comment; running the suite against a version of the
checker with `strip_noncode` replaced by the identity function is the
concrete way to see which ones catch it (the brace-in-string and both
trigger-in-{string,comment} cases all flip).
"""

from __future__ import annotations

import importlib.util
import sys
import tempfile
from pathlib import Path

SPEC = importlib.util.spec_from_file_location(
    "check_unfulfilled_expects",
    Path(__file__).resolve().parent / "check-unfulfilled-expects.py",
)
assert SPEC and SPEC.loader
CHECK = importlib.util.module_from_spec(SPEC)
# WHY: dataclasses' field-type resolution looks itself up via
# `sys.modules[cls.__module__]` -- exec_module() alone never registers that,
# so a plain module_from_spec load blows up inside the checker's own
# @dataclass decorators the moment it defines one.
sys.modules[SPEC.name] = CHECK
SPEC.loader.exec_module(CHECK)

FAILURES: list[str] = []


def check(path: str, text: str) -> list[tuple[int, str, str]]:
    """(line, lint, scope) for each violation, cross-file cfg check bypassed."""
    return sorted(
        (v.line, v.lint, v.scope)
        for v in CHECK.check_text(Path(path), text, skip_file_cfg_check=True)
    )


def expect_lints(label: str, path: str, text: str, want_lints: set[str]) -> None:
    got = {lint for _line, lint, _scope in check(path, text)}
    if got != want_lints:
        FAILURES.append(f"{label}: want lints {want_lints or '{}'}, got {got or '{}'}")


def expect_clean(label: str, path: str, text: str) -> None:
    expect_lints(label, path, text, set())


# --------------------------------------------------------------------------
# REAL: the two production incidents.
# --------------------------------------------------------------------------

# Verbatim `crates/graphe/src/store/fjall_store_tests_schema.rs` as it stood
# before the fix (commit 1f3db6583's parent): 33 `.expect(...)` calls, zero
# `.unwrap()`. `unwrap_used` is unfulfilled; `expect_used` is fulfilled and
# must NOT be flagged alongside it.
GRAPHE_SCHEMA_TESTS_BUGGY = '''\
#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(clippy::unwrap_used, reason = "test assertions")]

use tempfile::TempDir;

use super::super::{CURRENT_SCHEMA_VERSION, SCHEMA_MANIFEST_FILE, SchemaManifest, SessionStore};

fn manifest_path(store_path: &std::path::Path) -> std::path::PathBuf {
    store_path.join(SCHEMA_MANIFEST_FILE)
}

fn read_manifest(store_path: &std::path::Path) -> SchemaManifest {
    let bytes = std::fs::read(manifest_path(store_path)).expect("manifest file exists");
    serde_json::from_slice(&bytes).expect("manifest is valid JSON")
}

fn write_manifest_with_version(store_path: &std::path::Path, schema_version: u32) {
    let mut manifest = read_manifest(store_path);
    manifest.schema_version = schema_version;
    let data = serde_json::to_vec_pretty(&manifest).expect("manifest serializes");
    std::fs::write(manifest_path(store_path), data).expect("manifest overwrites");
}

#[test]
fn fresh_store_writes_schema_manifest() {
    let dir = TempDir::new().expect("temp dir creates");
    let path = dir.path().join("sessions");

    SessionStore::open(&path).expect("fresh store opens");

    let manifest = read_manifest(&path);
    assert_eq!(manifest.schema_version, CURRENT_SCHEMA_VERSION);
}

#[test]
fn missing_manifest_on_existing_store_refuses_and_preserves_data() {
    let dir = TempDir::new().expect("temp dir creates");
    let path = dir.path().join("sessions");

    {
        let store = SessionStore::open(&path).expect("first open");
        store
            .create_session("ses-legacy", "syn", "main", None, None)
            .expect("create session");
    }

    std::fs::remove_file(manifest_path(&path)).expect("manifest removed");

    let err = SessionStore::open(&path).expect_err("missing manifest on non-empty store refuses");
    let msg = err.to_string();
    assert!(msg.contains("no schema manifest"), "got: {msg}");

    SessionStore::stamp_legacy_schema_manifest(&path).expect("legacy stamp succeeds");
    let store = SessionStore::open(&path).expect("reopen after stamping succeeds");
    let restored = store
        .find_session_by_id("ses-legacy")
        .expect("query succeeds")
        .expect("session data survived the refusal untouched");
    assert_eq!(restored.id, "ses-legacy");
}

#[test]
fn corrupt_manifest_refuses_to_open() {
    let dir = TempDir::new().expect("temp dir creates");
    let path = dir.path().join("sessions");
    SessionStore::open(&path).expect("fresh store opens");
    std::fs::write(manifest_path(&path), b"not json").expect("manifest overwritten with garbage");

    let err = SessionStore::open(&path).expect_err("corrupt manifest refuses");
    assert!(err.to_string().contains("is corrupt"), "got: {err}");
}
'''

# Faithful trim of `crates/theatron/proskenion/src/api/system_status.rs`
# before the fix (commit 5276304e0's parent): `mod tests` propagates with
# `?` and asserts, never `.unwrap()`.
SYSTEM_STATUS_TESTS_BUGGY = '''\
async fn fetch_system_status(url: &str) -> Result<SystemStatusResponse, SystemStatusFetchError> {
    Ok(SystemStatusResponse::default())
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use std::error::Error;

    use super::*;

    async fn spawn_status_server() -> std::io::Result<String> {
        Ok("127.0.0.1:0".to_string())
    }

    #[test]
    fn failing_names_prefers_name_over_id() {
        let response = SystemStatusResponse::default();
        assert_eq!(response.status, "healthy");
    }

    #[tokio::test]
    async fn fetch_propagates_errors() -> Result<(), Box<dyn Error>> {
        let addr = spawn_status_server().await?;
        assert!(!addr.is_empty());
        Ok(())
    }
}
'''


def test_real_cases() -> None:
    expect_lints(
        "real: graphe fjall_store_tests_schema.rs (buggy)",
        "crates/graphe/src/store/fjall_store_tests_schema.rs",
        GRAPHE_SCHEMA_TESTS_BUGGY,
        {"unwrap_used"},
    )
    expect_lints(
        "real: proskenion system_status.rs (buggy)",
        "crates/theatron/proskenion/src/api/system_status.rs",
        SYSTEM_STATUS_TESTS_BUGGY,
        {"unwrap_used"},
    )


# --------------------------------------------------------------------------
# SYNTHETIC: fulfilled cases that must NOT be flagged.
# --------------------------------------------------------------------------


def test_fulfilled_mod_direct_unwrap() -> None:
    expect_clean(
        "fulfilled: direct .unwrap() in mod body",
        "src/a.rs",
        '''\
#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    #[test]
    fn parses_ok() {
        let v: u32 = "42".parse().unwrap();
        assert_eq!(v, 42);
    }
}
''',
    )


def test_fulfilled_by_err_variant() -> None:
    # Regression: clippy dispatches `.unwrap_err()` to the SAME `unwrap_used`
    # lint as `.unwrap()`, and `.expect_err()` to the same `expect_used` lint
    # as `.expect()` (rust-lang/rust-clippy#9338). Found live at
    # `crates/koina/src/error.rs`, whose `mod tests` calls `.unwrap_err()`
    # throughout and zero bare `.unwrap()` -- genuinely fulfilled, not a hit.
    expect_clean(
        "fulfilled: unwrap_used via .unwrap_err(), no bare .unwrap() at all",
        "src/a.rs",
        '''\
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    #[test]
    fn error_display_includes_path() {
        let err: Result<Vec<u8>, String> = Err("boom".to_string());
        let msg = err.unwrap_err();
        assert!(msg.contains("boom"));
    }
}
''',
    )
    expect_clean(
        "fulfilled: expect_used via .expect_err(), no bare .expect() at all",
        "src/a.rs",
        '''\
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    #[test]
    fn error_display_includes_path() {
        let err: Result<Vec<u8>, String> = Err("boom".to_string());
        let msg = err.expect_err("should be an error");
        assert!(msg.contains("boom"));
    }
}
''',
    )


def test_fulfilled_via_nested_module() -> None:
    # A naive implementation that stops at the first nested `mod { ... }`
    # block (treating it as "someone else's scope") instead of including
    # nested content in a flat trigger search would report this unfulfilled.
    # `#[expect]` on an outer item covers its descendants, same as `#[allow]`.
    expect_clean(
        "fulfilled: .unwrap() only inside a nested mod",
        "src/a.rs",
        '''\
#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    mod nested {
        #[test]
        fn deep_check() {
            let v: u32 = "7".parse().unwrap();
            assert_eq!(v, 7);
        }
    }

    #[test]
    fn shallow_check() {
        assert!(true);
    }
}
''',
    )


def test_fulfilled_past_a_brace_shaped_string() -> None:
    # A brace-counter that does not strip string contents first stops at the
    # stray `}` inside the string literal -- short-circuiting the mod's body
    # before it reaches the real `.unwrap()` a few lines later -- and reports
    # this unfulfilled. That is a false positive: this attribute IS fulfilled.
    expect_clean(
        "fulfilled: real .unwrap() sits after a brace-shaped string literal",
        "src/a.rs",
        '''\
#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    #[test]
    fn contains_a_brace_shaped_string() {
        let s = "not a real close brace }";
        assert_eq!(s.len(), 25);
    }

    #[test]
    fn really_uses_unwrap_after_the_stray_brace() {
        let v: u32 = "5".parse().unwrap();
        assert_eq!(v, 5);
    }
}
''',
    )


def test_fulfilled_file_level_no_expect_attrs() -> None:
    expect_clean(
        "fulfilled: file with no expect attributes at all",
        "src/plain.rs",
        '''\
fn add(a: u32, b: u32) -> u32 {
    a + b
}
''',
    )


# --------------------------------------------------------------------------
# SYNTHETIC: genuinely unfulfilled cases that MUST be caught.
# --------------------------------------------------------------------------


def test_unfulfilled_trigger_only_in_string_literal() -> None:
    # A raw substring search over unstripped text finds ".unwrap(" here and
    # wrongly concludes fulfilled. It is prose, not a call.
    expect_lints(
        "unfulfilled: trigger text only inside a string literal",
        "src/a.rs",
        '''\
#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    #[test]
    fn describes_the_pattern() {
        let doc = "call .unwrap() to panic on error";
        assert!(doc.contains("panic"));
    }
}
''',
        {"unwrap_used"},
    )


def test_unfulfilled_trigger_only_in_comment() -> None:
    expect_lints(
        "unfulfilled: trigger text only inside a comment",
        "src/a.rs",
        '''\
#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    // legacy code used to call .unwrap() here before the refactor
    #[test]
    fn refactored_to_use_question_mark() -> Result<(), String> {
        let v: u32 = "9".parse().map_err(|e: std::num::ParseIntError| e.to_string())?;
        assert_eq!(v, 9);
        Ok(())
    }
}
''',
        {"unwrap_used"},
    )


def test_unfulfilled_stacked_attributes_independent() -> None:
    expect_lints(
        "unfulfilled: two stacked #[expect] attrs evaluated independently",
        "src/a.rs",
        '''\
#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    #[test]
    fn only_uses_expect() {
        let v: u32 = "3".parse().expect("valid digit");
        assert_eq!(v, 3);
    }
}
''',
        {"unwrap_used"},
    )


def test_unfulfilled_combined_multi_lint_attribute() -> None:
    expect_lints(
        "unfulfilled: one attribute naming two lints, evaluated independently",
        "src/a.rs",
        '''\
#[cfg(test)]
#[expect(clippy::unwrap_used, clippy::expect_used, reason = "test assertions")]
mod tests {
    #[test]
    fn only_uses_expect() {
        let v: u32 = "3".parse().expect("valid digit");
        assert_eq!(v, 3);
    }
}
''',
        {"unwrap_used"},
    )


def test_unfulfilled_panic_lint() -> None:
    expect_lints(
        "unfulfilled: clippy::panic trigger",
        "src/a.rs",
        '''\
#[expect(clippy::panic, reason = "test assertions")]
mod tests {
    #[test]
    fn never_panics() {
        assert!(true);
    }
}
''',
        {"panic"},
    )


def test_unfulfilled_inner_attribute_just_inside_mod() -> None:
    # `#![expect(...)]` as the first statement inside `mod tests { ... }`
    # scopes to that mod's body, same as an outer `#[expect]` before it.
    expect_lints(
        "unfulfilled: #![expect] as first line inside mod tests {",
        "src/a.rs",
        '''\
#[cfg(test)]
mod tests {
    #![expect(clippy::unwrap_used, reason = "test assertions")]

    #[test]
    fn never_unwraps() {
        assert!(true);
    }
}
''',
        {"unwrap_used"},
    )


# --------------------------------------------------------------------------
# SYNTHETIC: cfg gating -- excluded rather than flagged.
# --------------------------------------------------------------------------


def test_excluded_feature_gated_mod() -> None:
    # Genuinely no `.unwrap()` in the body, but the mod is only compiled
    # under a feature this check cannot confirm is on -- must not flag.
    expect_clean(
        "excluded: mod gated behind a feature flag",
        "src/a.rs",
        '''\
#[cfg(test)]
#[cfg(feature = "storage-fjall")]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    #[test]
    fn only_runs_with_feature() {
        assert!(true);
    }
}
''',
    )


def test_not_excluded_any_test_or_feature() -> None:
    # `any(test, feature = "x")` is true whenever `test` is, regardless of
    # the feature -- test-safe, so this one MUST still be evaluated.
    expect_lints(
        "not excluded: any(test, feature) reduces to test-safe",
        "src/a.rs",
        '''\
#[cfg(any(test, feature = "storage-fjall"))]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    #[test]
    fn always_compiled_under_test() {
        assert!(true);
    }
}
''',
        {"unwrap_used"},
    )


def test_excluded_all_test_and_feature_either_order() -> None:
    # Regression guard for a token-accounting bug in the cfg evaluator: a
    # `key = "value"` atom's value is blanked to nothing by strip_noncode,
    # so consuming a token unconditionally after `=` ate the next real
    # token (a `,` or `)`) and desynchronized parsing of everything after
    # it. Order matters for exposing it: `feature` must appear before the
    # atom that would otherwise prove the whole expression true/false.
    expect_clean(
        "excluded: all(test, feature) -- feature first",
        "src/a.rs",
        '''\
#[cfg(all(feature = "storage-fjall", test))]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    #[test]
    fn gated() {
        assert!(true);
    }
}
''',
    )
    expect_lints(
        "not excluded: any(feature, test) -- feature first",
        "src/a.rs",
        '''\
#[cfg(any(feature = "storage-fjall", test))]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    #[test]
    fn always_compiled_under_test() {
        assert!(true);
    }
}
''',
        {"unwrap_used"},
    )


# --------------------------------------------------------------------------
# SYNTHETIC: out of the reliable subset -- must be silently excluded.
# --------------------------------------------------------------------------


def test_excluded_function_level_expect() -> None:
    expect_clean(
        "excluded: #[expect] on a fn, not a mod (out of reliable subset)",
        "src/a.rs",
        '''\
#[expect(clippy::unwrap_used, reason = "encoding into String is infallible")]
fn always_unwraps_safely(input: &str) -> String {
    input.to_string()
}
''',
    )


def test_excluded_unsupported_lint() -> None:
    expect_clean(
        "excluded: indexing_slicing is not in the reliable subset",
        "src/a.rs",
        '''\
#[cfg(test)]
#[expect(clippy::indexing_slicing, reason = "test assertions on Vecs with asserted length")]
mod tests {
    #[test]
    fn never_indexes() {
        assert!(true);
    }
}
''',
    )


def test_excluded_deny_is_not_expect() -> None:
    # Regression: `#![deny(clippy::unwrap_used, clippy::expect_used)]` is a
    # codebase-wide policy line, not an expectation -- `deny` never
    # participates in unfulfilled-lint-expectations. A `clippy::LINT` regex
    # match inside its bracket span must not be read as an expect-attribute
    # lint name (found live at crates/theatron/koilon/src/lib.rs:3, whose
    # zero `.expect(` calls in the whole file made this look identical to a
    # real file-level unfulfilled `expect_used` until traced back).
    expect_clean(
        "excluded: #![deny(...)] is not #![expect(...)]",
        "src/a.rs",
        '''\
#![deny(clippy::unwrap_used, clippy::expect_used)]

fn add(a: u32, b: u32) -> u32 {
    a + b
}
''',
    )
    expect_clean(
        "excluded: #![warn(...)]/#![allow(...)] are not #![expect(...)] either",
        "src/a.rs",
        '''\
#![warn(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

fn add(a: u32, b: u32) -> u32 {
    a + b
}
''',
    )


# --------------------------------------------------------------------------
# File-level cross-file cfg resolution (real filesystem, one-hop).
# --------------------------------------------------------------------------


def _write_crate(root: Path, lib_rs: str, extra: dict[str, str]) -> None:
    (root / "Cargo.toml").write_text('[package]\nname = "fixture"\nversion = "0.0.0"\n')
    src = root / "src"
    src.mkdir()
    (src / "lib.rs").write_text(lib_rs)
    for name, content in extra.items():
        (src / name).write_text(content)


UNFULFILLED_FILE_LEVEL = '''\
#![expect(clippy::unwrap_used, reason = "test assertions")]

fn add(a: u32, b: u32) -> u32 {
    a + b
}
'''


def test_file_level_excluded_when_declared_behind_a_feature() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        _write_crate(
            root,
            '#[cfg(feature = "extra")]\nmod gated;\n',
            {"gated.rs": UNFULFILLED_FILE_LEVEL},
        )
        target = root / "src" / "gated.rs"
        got = CHECK.check_text(target, UNFULFILLED_FILE_LEVEL)
        if got:
            FAILURES.append(
                f"file-level cross-file: expected exclusion behind #[cfg(feature)], got {got}"
            )


def test_file_level_flagged_when_declared_unconditionally() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        _write_crate(
            root,
            "mod clear;\n",
            {"clear.rs": UNFULFILLED_FILE_LEVEL},
        )
        target = root / "src" / "clear.rs"
        got = CHECK.check_text(target, UNFULFILLED_FILE_LEVEL)
        if len(got) != 1 or got[0].lint != "unwrap_used":
            FAILURES.append(
                f"file-level cross-file: expected one unwrap_used violation, got {got}"
            )


def test_file_level_flagged_when_declaration_not_found() -> None:
    # No sibling declares this file at all (the common shape for a
    # `tests/*.rs` integration-test binary cargo auto-discovers) -- default
    # to unconditionally reachable, per the documented policy.
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        (root / "Cargo.toml").write_text('[package]\nname = "fixture"\nversion = "0.0.0"\n')
        tests_dir = root / "tests"
        tests_dir.mkdir()
        target = tests_dir / "smoke.rs"
        target.write_text(UNFULFILLED_FILE_LEVEL)
        got = CHECK.check_text(target, UNFULFILLED_FILE_LEVEL)
        if len(got) != 1 or got[0].lint != "unwrap_used":
            FAILURES.append(
                f"file-level cross-file: expected a violation when no declaring mod is found, got {got}"
            )


def test_file_level_fulfilled_by_umbrella_child_module() -> None:
    # Real shape: crates/daemon/src/runner_tests/mod.rs carries the
    # `#![expect]` pair, declares four child test modules, and contains no
    # test code of its own. Each child lives in its own file (the
    # `RUST/file-too-long` split convention) and one of them genuinely
    # calls `.unwrap()`. The umbrella file's OWN text must not be the
    # search boundary -- an #[expect] on a module reaches its descendants
    # regardless of which file their text lives in.
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        _write_crate(root, "mod runner_tests;\n", {})
        runner_tests_dir = root / "src" / "runner_tests"
        runner_tests_dir.mkdir()
        umbrella = '''\
#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(clippy::unwrap_used, reason = "test assertions")]

mod cron_and_output;
mod lifecycle_and_builders;
'''
        (runner_tests_dir / "mod.rs").write_text(umbrella)
        (runner_tests_dir / "cron_and_output.rs").write_text(
            "#[test]\nfn runs_cron() { assert!(true); }\n"
        )
        (runner_tests_dir / "lifecycle_and_builders.rs").write_text(
            '#[test]\nfn builds() { let v: u32 = "9".parse().unwrap(); assert_eq!(v, 9); }\n'
        )
        target = runner_tests_dir / "mod.rs"
        got = CHECK.check_text(target, umbrella)
        # unwrap_used is fulfilled by lifecycle_and_builders.rs;
        # expect_used has no `.expect(`/`.expect_err(` anywhere in the tree
        # and must still be flagged.
        lints = {v.lint for v in got}
        if lints != {"expect_used"}:
            FAILURES.append(
                f"umbrella mod.rs: expected only expect_used flagged, got lints={lints} ({got})"
            )


# --------------------------------------------------------------------------
# strip_noncode: the primitive everything above depends on.
# --------------------------------------------------------------------------


def test_strip_noncode() -> None:
    # Braces inside a string are blanked (line length preserved).
    stripped = CHECK.strip_noncode('let s = "a { b } c";')
    if "{" in stripped or "}" in stripped:
        FAILURES.append(f"strip_noncode: brace survived inside a string: {stripped!r}")
    if len(stripped) != len('let s = "a { b } c";'):
        FAILURES.append("strip_noncode: length changed for a string literal")

    # Line comments are blanked but the newline is preserved.
    stripped = CHECK.strip_noncode("let x = 1; // .unwrap( is just a comment\nlet y = 2;")
    if ".unwrap(" in stripped:
        FAILURES.append(f"strip_noncode: trigger text survived inside a line comment: {stripped!r}")
    if stripped.count("\n") != 1:
        FAILURES.append("strip_noncode: line comment blanking changed the newline count")

    # Nested block comments are fully consumed.
    stripped = CHECK.strip_noncode("/* outer /* inner } */ still comment */ code_after")
    if "}" in stripped:
        FAILURES.append(f"strip_noncode: brace survived inside a nested block comment: {stripped!r}")
    if "code_after" not in stripped:
        FAILURES.append(f"strip_noncode: code after a nested block comment was blanked: {stripped!r}")

    # Raw strings with hash delimiters are fully blanked, hash count matched.
    stripped = CHECK.strip_noncode('let s = r#"has a "quote" and a } inside"#;')
    if "}" in stripped:
        FAILURES.append(f"strip_noncode: brace survived inside a raw string: {stripped!r}")

    # Char literal vs lifetime: a char literal is blanked, a lifetime is not.
    stripped = CHECK.strip_noncode("fn f<'a>(c: char) { let x = '{'; }")
    # The lifetime's `'a` must survive (still code); the char literal `'{'`
    # must not leave its brace exposed to the structural brace counter.
    if "'a" not in stripped:
        FAILURES.append(f"strip_noncode: lifetime tick was incorrectly blanked: {stripped!r}")
    # Count real (non-literal) braces: only the function body's `{` and `}`.
    real_braces = stripped.count("{") + stripped.count("}")
    if real_braces != 2:
        FAILURES.append(
            f"strip_noncode: expected exactly 2 structural braces (fn body), got {real_braces}: {stripped!r}"
        )


# --------------------------------------------------------------------------
# cfg_is_test_safe: the propositional evaluator in isolation.
# --------------------------------------------------------------------------


def test_cfg_is_test_safe() -> None:
    safe = [
        "test",
        'any(test, feature = "x")',
        'any(feature = "x", test)',
        'all(test, any(test, feature = "x"))',
    ]
    unsafe = [
        'feature = "x"',
        'all(test, feature = "x")',
        'all(feature = "x", test)',
        "unix",
        'not(test)',
        'all(test, not(feature = "x"))',
    ]
    for predicate in safe:
        if not CHECK.cfg_is_test_safe(predicate):
            FAILURES.append(f"cfg_is_test_safe: expected True for {predicate!r}")
    for predicate in unsafe:
        if CHECK.cfg_is_test_safe(predicate):
            FAILURES.append(f"cfg_is_test_safe: expected False for {predicate!r}")


def main() -> int:
    tests = [v for k, v in sorted(globals().items()) if k.startswith("test_") and callable(v)]
    for t in tests:
        t()

    if FAILURES:
        for f in FAILURES:
            print(f"FAIL: {f}", file=sys.stderr)
        print(f"\n{len(FAILURES)} failure(s) across {len(tests)} test functions", file=sys.stderr)
        return 1

    print(f"OK: {len(tests)} test functions passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
