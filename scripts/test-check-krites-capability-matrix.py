#!/usr/bin/env python3
"""Behavioral tests for scripts/check-krites-capability-matrix.py + krites_test_index.py.

The matrix's new enforcement is only worth its cost if the failure modes it
claims to catch actually fail. Each test below stages one of them against the
LIVE matrix rows and asserts a specific error, so a future refactor that
silently turns a check into a no-op is caught here rather than by nobody:

  a gate_test naming a test that does not exist        -> error
  a gate_test naming an #[ignore]d test                -> error
  a capability row deleted                             -> UNMAPPED
  a capability_set member deleted from the record      -> UNRECORDED
  a recorded member that source no longer has          -> DROPPED
  a source citation pointing at the wrong line         -> error
  the whole capability_set block deleted               -> error

The test-index tests use fixture crates written to a temp dir, because the
walker's hard cases (`#[path]`, an inline `mod`, a `mod` name appearing inside
a string literal) are exactly the ones the real tree happens not to exercise in
isolation.
"""

from __future__ import annotations

import copy
import importlib.util
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import krites_test_index as KTI  # noqa: E402

_CHECK_SCRIPT = Path(__file__).resolve().parent / "check-krites-capability-matrix.py"


def _load(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"failed to load {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


CHECKER = _load("check_krites_capability_matrix", _CHECK_SCRIPT)
LIVE_ROWS = CHECKER.load_matrix()
LIVE_SETS = CHECKER.load_capability_sets()

_failures: list[str] = []


def check(name: str, condition: bool, detail: str = "") -> None:
    if condition:
        print(f"  ok   {name}")
    else:
        print(f"  FAIL {name}: {detail}")
        _failures.append(name)


def _rows_with(**overrides) -> list[dict]:
    """Copy the live rows, applying `{row_id: {field: value}}` overrides."""
    rows = copy.deepcopy(LIVE_ROWS)
    for row in rows:
        patch = overrides.get(row.get("id"))
        if patch:
            row.update(patch)
    return rows


def test_live_matrix_is_green() -> None:
    errors = CHECKER.check_all_rows_well_formed(LIVE_ROWS)
    errors += CHECKER.check_capability_sets(LIVE_SETS)
    errors += CHECKER.check_file_line_refs(LIVE_ROWS)
    gate_errors, _, pointed, unpointed = CHECKER.check_gate_tests(LIVE_ROWS)
    errors += gate_errors
    check("live matrix has no structural errors", not errors, str(errors[:3]))
    check("live matrix has at least one pointed row", pointed > 0, f"pointed={pointed}")
    check(
        "pointed + unpointed accounts for every row",
        pointed + unpointed == len(LIVE_ROWS),
        f"{pointed} + {unpointed} != {len(LIVE_ROWS)}",
    )


def test_gate_test_naming_a_missing_test_fails() -> None:
    target = next(r["id"] for r in LIVE_ROWS if r.get("gate_test") not in (None, "none"))
    rows = _rows_with(**{target: {"gate_test": "krites::this::test::does::not::exist"}})
    errors, _, _, _ = CHECKER.check_gate_tests(rows)
    check(
        "gate_test naming no test is an error",
        any("names no test" in e for e in errors),
        str(errors[:2]),
    )


def test_gate_test_naming_an_ignored_test_fails() -> None:
    target = next(r["id"] for r in LIVE_ROWS if r.get("gate_test") not in (None, "none"))
    pointer = next(r["gate_test"] for r in LIVE_ROWS if r["id"] == target)
    real_build = KTI.build_index

    def fake_build(crate_dir, repo_root):
        index, unresolved = real_build(crate_dir, repo_root)
        case = index[pointer]
        index[pointer] = KTI.TestCase(
            binary_id=case.binary_id,
            test_path=case.test_path,
            ignored=True,
            file=case.file,
            line=case.line,
            cfg_guards=case.cfg_guards,
        )
        return index, unresolved

    KTI.build_index = fake_build
    try:
        errors, _, _, _ = CHECKER.check_gate_tests(copy.deepcopy(LIVE_ROWS))
    finally:
        KTI.build_index = real_build
    check(
        "gate_test naming an #[ignore]d test is an error",
        any("#[ignore]d" in e for e in errors),
        str(errors[:2]),
    )


def test_none_pointer_counts_as_unpointed_not_as_an_error() -> None:
    target = next(r["id"] for r in LIVE_ROWS if r.get("gate_test") not in (None, "none"))
    _, _, base_pointed, _ = CHECKER.check_gate_tests(copy.deepcopy(LIVE_ROWS))
    rows = _rows_with(**{target: {"gate_test": "none"}})
    errors, _, pointed, _ = CHECKER.check_gate_tests(rows)
    check("\"none\" is not an error", not errors, str(errors[:2]))
    check('"none" reduces the pointed count', pointed == base_pointed - 1, f"{pointed}")


def test_deleting_a_graph_algorithm_row_fails() -> None:
    rows = [r for r in copy.deepcopy(LIVE_ROWS) if r.get("id") != "fixed-rule-page-rank"]
    errors = CHECKER.check_category(
        "fixed_rule", CHECKER.extract_fixed_rule_names(), rows, "fixed_rule/mod.rs"
    )
    check(
        "deleting a fixed_rule row is UNMAPPED",
        any("UNMAPPED [fixed_rule] PageRank" in e for e in errors),
        str(errors[:2]),
    )


def test_deleting_a_storage_method_row_fails() -> None:
    rows = [r for r in copy.deepcopy(LIVE_ROWS) if r.get("id") != "store-tx-del-range-from-persisted"]
    errors = CHECKER.check_category(
        "storage_method", CHECKER.extract_storage_methods(), rows, "storage/mod.rs"
    )
    check(
        "deleting a storage_method row is UNMAPPED",
        any("StoreTx::del_range_from_persisted" in e for e in errors),
        str(errors[:2]),
    )


def test_capability_set_drift_fails_in_both_directions() -> None:
    sets = copy.deepcopy(LIVE_SETS)
    target = next(s for s in sets if s["id"] == "scalar-functions")
    dropped = target["members"].pop()
    errors = CHECKER.check_capability_sets(sets)
    check(
        "a source member missing from the record is UNRECORDED",
        any(f"UNRECORDED [capability_set scalar-functions] '{dropped}'" in e for e in errors),
        str(errors[:2]),
    )

    sets = copy.deepcopy(LIVE_SETS)
    target = next(s for s in sets if s["id"] == "aggregations")
    target["members"] = sorted([*target["members"], "AGGR_INVENTED"])
    errors = CHECKER.check_capability_sets(sets)
    check(
        "a recorded member absent from source is DROPPED",
        any("DROPPED [capability_set aggregations] 'AGGR_INVENTED'" in e for e in errors),
        str(errors[:2]),
    )


def test_deleting_a_whole_capability_set_fails() -> None:
    sets = [s for s in copy.deepcopy(LIVE_SETS) if s["id"] != "aggregations"]
    errors = CHECKER.check_capability_sets(sets)
    check(
        "deleting a whole set is an error",
        any("has a source derivation but no row" in e for e in errors),
        str(errors[:2]),
    )


def test_citation_pointing_at_the_wrong_line_fails() -> None:
    rows = _rows_with(**{"api-db-run": {"source": "crates/krites/src/lib.rs:1"}})
    errors = CHECKER.check_file_line_refs(rows)
    check(
        "an in-range citation that names nothing is an error",
        any("names none of the row's item tokens" in e for e in errors),
        str(errors[:2]),
    )


def _write_fixture(root: Path) -> Path:
    crate = root / "fixture"
    (crate / "src" / "inner").mkdir(parents=True)
    (crate / "tests").mkdir(parents=True)
    (crate / "Cargo.toml").write_text('[package]\nname = "fixture"\nversion = "0.1.0"\n')
    (crate / "src" / "lib.rs").write_text(
        "pub mod inner;\n"
        '#[path = "aliased.rs"]\n'
        "mod renamed;\n"
        "#[cfg(test)]\n"
        "mod tests {\n"
        "    #[test]\n"
        "    fn top_level() {}\n"
        "    #[test]\n"
        "    #[ignore = \"slow\"]\n"
        "    fn skipped() {}\n"
        "    #[test]\n"
        "    fn holds_a_script() {\n"
        '        let _ = r#"mod fake; #[test] fn invented() {}"#;\n'
        "    }\n"
        "}\n"
    )
    (crate / "src" / "aliased.rs").write_text("#[test]\nfn reached_via_path_attr() {}\n")
    (crate / "src" / "inner" / "mod.rs").write_text(
        "#[cfg(test)]\nmod deep {\n    #[tokio::test]\n    async fn async_case() {}\n}\n"
    )
    (crate / "tests" / "it.rs").write_text("#[test]\nfn integration_case() {}\n")
    return crate


def test_test_index_walks_the_hard_shapes() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        crate = _write_fixture(root)
        index, unresolved = KTI.build_index(crate, root)
        check("fixture has no unresolved modules", not unresolved, str(unresolved))
        expected = {
            "fixture::tests::top_level",
            "fixture::tests::skipped",
            "fixture::tests::holds_a_script",
            "fixture::renamed::reached_via_path_attr",
            "fixture::inner::deep::async_case",
            "fixture::it::integration_case",
        }
        check("index finds exactly the real tests", set(index) == expected, str(set(index) ^ expected))
        check("#[ignore] is read", index["fixture::tests::skipped"].ignored)
        check("non-ignored tests are not marked ignored", not index["fixture::tests::top_level"].ignored)
        check(
            "a `mod` inside a raw string invents nothing",
            not any("fake" in tid or "invented" in tid for tid in index),
            str(sorted(index)),
        )


def test_strip_noise_preserves_length_and_blanks_literals() -> None:
    src = 'let s = "mod fake;"; // mod also_fake;\n/* mod block_fake; */\nfn real() {}\n'
    stripped = KTI.strip_noise(src)
    check("strip_noise preserves length", len(stripped) == len(src))
    check("strip_noise removes literal content", "fake" not in stripped, stripped)
    check("strip_noise keeps code", "fn real() {}" in stripped, stripped)
    check(
        "strip_noise preserves newlines",
        stripped.count("\n") == src.count("\n"),
        f"{stripped.count(chr(10))} != {src.count(chr(10))}",
    )


def main() -> int:
    for name, fn in sorted(globals().items()):
        if name.startswith("test_") and callable(fn):
            print(name)
            fn()
    if _failures:
        print(f"\n{len(_failures)} check(s) failed: {_failures}", file=sys.stderr)
        return 1
    print("\nall checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
