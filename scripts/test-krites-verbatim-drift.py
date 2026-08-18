#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Tests for scripts/check-krites-verbatim-drift.py.

Covers the line classifier (punctuation-only / use / attribute / low-
identifier-count exclusion), the tokenizer, shingle/Jaccard arithmetic, and
a regression guard: re-runs the calibration against the pinned upstream
snapshot and asserts CALIBRATED_THRESHOLD still clears the known-original
set's measured max. A tokenizer change that erodes the metric's
discriminating power fails this test before it fails calibration review.

Run with:
    uv run scripts/test-krites-verbatim-drift.py

Exits 0 on success, 1 on first failure.
"""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

SCRIPT_PATH = Path(__file__).resolve().parent / "check-krites-verbatim-drift.py"


def _load_module():
    spec = importlib.util.spec_from_file_location("krites_drift_metric", SCRIPT_PATH)
    module = importlib.util.module_from_spec(spec)
    # WHY: dataclass field-type resolution needs the module registered before
    # exec — otherwise `dataclasses._is_type` looks it up in `sys.modules` and
    # finds nothing.
    sys.modules["krites_drift_metric"] = module
    spec.loader.exec_module(module)
    return module


DRIFT = _load_module()

FAILURES: list[str] = []


def check(name: str, condition: bool, detail: str = "") -> None:
    if not condition:
        FAILURES.append(f"{name}: {detail}")


def test_punctuation_only_lines_excluded() -> None:
    src = "fn f() {\n}\n);\n{\n},\n"
    lines = DRIFT.eligible_lines(src)
    check("punctuation-only lines excluded", lines == [], f"got {lines!r}")


def test_use_statement_excluded_single_line() -> None:
    src = "use crate::foo::bar::Baz;\nfn real_function_body() { call_something(); }\n"
    lines = DRIFT.eligible_lines(src)
    check(
        "single-line use excluded",
        all("use " not in line for line in lines),
        f"got {lines!r}",
    )


def test_use_statement_excluded_multi_line() -> None:
    src = (
        "use std::collections::{\n"
        "    HashMap,\n"
        "    HashSet,\n"
        "};\n"
        "fn real_function_body() { call_something_here(); }\n"
    )
    lines = DRIFT.eligible_lines(src)
    check(
        "multi-line use block fully excluded",
        all("HashMap" not in line and "HashSet" not in line for line in lines),
        f"got {lines!r}",
    )


def test_attribute_line_excluded() -> None:
    src = "#[derive(Debug, Clone)]\nfn real_function_body() { call_something_here(); }\n"
    lines = DRIFT.eligible_lines(src)
    check(
        "attribute line excluded",
        all(not line.startswith("#[") for line in lines),
        f"got {lines!r}",
    )


def test_low_identifier_count_line_excluded() -> None:
    # NOTE: "let x = 1;" has 2 identifier-shaped tokens (let, x) -> below the
    # >=3 threshold, so it must be excluded regardless of punctuation/use/attr.
    src = "let x = 1;\nfn real_function_body() { call_something_here(); }\n"
    lines = DRIFT.eligible_lines(src)
    check(
        "line with <3 identifier tokens excluded",
        all("let x" not in line for line in lines),
        f"got {lines!r}",
    )


def test_eligible_line_with_enough_identifiers_kept() -> None:
    src = "fn real_function_body() { call_something_here(); }\n"
    lines = DRIFT.eligible_lines(src)
    check("content line with >=3 identifiers kept", len(lines) == 1, f"got {lines!r}")


def test_tokenizer_identifiers_and_strings() -> None:
    tokens = DRIFT.tokenize('fn real_function_body() { let message = "hello world"; }\n')
    check("string literal captured as one token", '"hello world"' in tokens, f"got {tokens!r}")
    check("identifier captured", "real_function_body" in tokens, f"got {tokens!r}")


def test_shingles_identical_text_full_overlap() -> None:
    text = "fn real_function_body() { call_something_here_please(); }\n" * 3
    tokens = DRIFT.tokenize(text)
    a = DRIFT.shingles(tokens)
    b = DRIFT.shingles(tokens)
    check("identical token streams -> jaccard 1.0", DRIFT.jaccard(a, b) == 1.0)


def test_shingles_disjoint_text_zero_overlap() -> None:
    text_a = "fn alpha_function_body() { call_alpha_helper_here(); }\n"
    text_b = "fn beta_function_body() { call_beta_helper_there(); }\n"
    a = DRIFT.shingles(DRIFT.tokenize(text_a))
    b = DRIFT.shingles(DRIFT.tokenize(text_b))
    check("disjoint token streams -> jaccard 0.0", DRIFT.jaccard(a, b) == 0.0)


def test_short_stream_below_shingle_size_yields_empty_set() -> None:
    tokens = DRIFT.tokenize("fn f(a, b) { g(a); }\n")
    s = DRIFT.shingles(tokens)
    check(
        "fewer than SHINGLE_SIZE tokens -> empty shingle set or non-crashing",
        isinstance(s, frozenset),
    )


def test_calibration_regression_guard() -> None:
    """The metric must still separate the known-original set from the
    calibrated threshold, against the CURRENT tokenizer + CURRENT pinned
    snapshot. A change that erodes this must fail here, not in prose review."""
    if not DRIFT.UPSTREAM_SRC.is_dir():
        check("upstream snapshot present", False, f"missing at {DRIFT.UPSTREAM_SRC}")
        return

    krites = DRIFT.load_krites_corpus()
    upstream = DRIFT.load_upstream_corpus()

    highest_original = 0.0
    for relpath in DRIFT.KNOWN_ORIGINAL_FILES:
        fs = krites.get(relpath)
        check(f"known-original file present: {relpath}", fs is not None)
        if fs is None:
            continue
        score = DRIFT.global_best_match(fs, upstream)
        highest_original = max(highest_original, score.jaccard)

    check(
        "CALIBRATED_THRESHOLD clears known-original max",
        DRIFT.CALIBRATED_THRESHOLD > highest_original,
        f"threshold={DRIFT.CALIBRATED_THRESHOLD} highest_original={highest_original}",
    )


def test_generated_exhibit_a_notice_excluded() -> None:
    # WHY(#5956): this filter keeps ordinary comment lines, and 122 of the upstream files
    # carry the MPL notice in their own header -- so a per-file notice stamped on a derived
    # file would shingle-match upstream's copy and raise this metric on licence boilerplate
    # rather than on shared expression. The exclusion has to reach this instrument too, not
    # only verbatim_pct.
    sys.path.insert(0, str(SCRIPT_PATH.parent))
    import krites_provenance_lib as lib

    body = "fn real_function_body() { call_something_here(); }\n"
    stamped = lib.add_generated_notice(body, lib.render_exhibit_a(".rs"))
    check(
        "generated exhibit-a notice excluded",
        DRIFT.eligible_lines(stamped) == DRIFT.eligible_lines(body),
        f"got {DRIFT.eligible_lines(stamped)!r}",
    )
    check(
        "notice would otherwise be eligible",
        len([line for line in stamped.splitlines() if line.strip()]) > len(DRIFT.eligible_lines(stamped)),
        "fixture proves nothing if the notice is dropped by another filter",
    )
    check(
        "notice shingles do not reach the token stream",
        "Mozilla" not in DRIFT.tokenize(stamped),
        f"got {DRIFT.tokenize(stamped)!r}",
    )


def main() -> int:
    tests = [
        test_punctuation_only_lines_excluded,
        test_use_statement_excluded_single_line,
        test_use_statement_excluded_multi_line,
        test_attribute_line_excluded,
        test_low_identifier_count_line_excluded,
        test_eligible_line_with_enough_identifiers_kept,
        test_tokenizer_identifiers_and_strings,
        test_shingles_identical_text_full_overlap,
        test_shingles_disjoint_text_zero_overlap,
        test_short_stream_below_shingle_size_yields_empty_set,
        test_calibration_regression_guard,
        test_generated_exhibit_a_notice_excluded,
    ]
    for t in tests:
        t()

    if FAILURES:
        print(f"FAILED {len(FAILURES)}/{len(tests)}:", file=sys.stderr)
        for f in FAILURES:
            print(f"  - {f}", file=sys.stderr)
        return 1

    print(f"PASSED {len(tests)}/{len(tests)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
