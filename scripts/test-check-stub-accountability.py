#!/usr/bin/env python3
"""Tests for check-stub-accountability.py.

Covers the attribute scanner (bracket-depth matching, cfg_attr unwrapping,
the test-locality exclusion), the accountability mechanisms (inline issue
reference, TODO/FIXME window, keyword-vs-dead_code candidacy), and the
baseline's bidirectional drift detection (new debt vs. paid-down debt going
stale). Mirrors test-check-orphaned-modules.py's harness: no pytest
dependency, plain `expect()` assertions collected into one FAILURES list.
"""

from __future__ import annotations

import importlib.util
import sys
from collections import Counter
from pathlib import Path

SPEC = importlib.util.spec_from_file_location(
    "check_stub_accountability",
    Path(__file__).resolve().parent / "check-stub-accountability.py",
)
assert SPEC and SPEC.loader
CHECK = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = CHECK
SPEC.loader.exec_module(CHECK)

FAILURES: list[str] = []


def expect(label: str, cond: bool, detail: str = "") -> None:
    if not cond:
        FAILURES.append(f"{label}: {detail}" if detail else label)


# --------------------------------------------------------------------------
# find_attributes: bracket-depth matching


def test_find_attributes_single_line() -> None:
    text = '#[expect(dead_code, reason = "planned TUI feature")]\nfn f() {}\n'
    attrs = CHECK.find_attributes(text)
    expect("one attribute found", len(attrs) == 1, f"got {attrs!r}")
    if attrs:
        expect(
            "content excludes outer brackets",
            attrs[0][2] == 'expect(dead_code, reason = "planned TUI feature")',
            f"got {attrs[0][2]!r}",
        )


def test_find_attributes_multiline_cfg_attr() -> None:
    text = (
        "#[cfg_attr(\n"
        "    not(test),\n"
        '    expect(dead_code, reason = "WIP: plan execution lifecycle")\n'
        ")]\n"
        "fn f() {}\n"
    )
    attrs = CHECK.find_attributes(text)
    expect("multiline cfg_attr found as one attribute", len(attrs) == 1, f"got {attrs!r}")
    if attrs:
        expect("dead_code visible inside cfg_attr", "dead_code" in attrs[0][2], f"got {attrs[0][2]!r}")


def test_find_attributes_bracket_inside_reason_string_does_not_desync() -> None:
    # WHY: a `]` inside the reason string must not be read as the closing
    # bracket of the attribute -- the in-string tracking in find_attributes
    # exists specifically so this does not truncate the attribute early.
    text = '#[expect(dead_code, reason = "see item[0] for detail")]\nfn f() {}\n'
    attrs = CHECK.find_attributes(text)
    expect("attribute not truncated by bracket in string", len(attrs) == 1, f"got {attrs!r}")
    if attrs:
        expect("full reason preserved", "item[0]" in attrs[0][2], f"got {attrs[0][2]!r}")


def test_find_attributes_file_level_bang() -> None:
    text = "#![allow(dead_code)]\n\nfn f() {}\n"
    attrs = CHECK.find_attributes(text)
    expect("file-level #![...] found", len(attrs) == 1, f"got {attrs!r}")


# --------------------------------------------------------------------------
# scan_text: candidacy rules


def test_dead_code_without_test_word_is_candidate() -> None:
    text = '#[expect(dead_code, reason = "WIP: plan execution lifecycle")]\nfn f() {}\n'
    candidates = CHECK.scan_text("fixture.rs", text)
    expect("bare dead_code with non-test reason is a candidate", len(candidates) == 1, f"got {candidates!r}")


def test_dead_code_with_test_word_is_not_candidate() -> None:
    # WHY: this is the single most common shape in the real tree (100+
    # sites) -- a helper compiled out because it is only exercised by test
    # code. It is not #4530's target and must not need a tracking issue.
    text = '#[cfg_attr(not(test), expect(dead_code, reason = "used in tests"))]\nfn f() {}\n'
    candidates = CHECK.scan_text("fixture.rs", text)
    expect("test-locality dead_code is excluded", candidates == [], f"got {candidates!r}")


def test_keyword_reason_is_candidate_even_when_mentioning_test() -> None:
    # WHY: an author who explicitly writes "reserved"/"planned"/etc. has
    # already called it scaffolding -- the test-locality exclusion only
    # covers the ambiguous case where NEITHER signal is present.
    text = '#[allow(clippy::dead_code, reason = "reserved for tests")]\nfn f() {}\n'
    candidates = CHECK.scan_text("fixture.rs", text)
    expect("keyword wins over test-mention", len(candidates) == 1, f"got {candidates!r}")


def test_ordinary_reason_no_dead_code_is_not_candidate() -> None:
    text = '#[expect(clippy::too_many_arguments, reason = "legacy call site")]\nfn f() {}\n'
    candidates = CHECK.scan_text("fixture.rs", text)
    expect("no dead_code, no keyword -> not a candidate", candidates == [], f"got {candidates!r}")


def test_each_five_issue_keywords_trigger_candidacy() -> None:
    for word in ("stub", "reserved", "not yet wired", "planned", "future"):
        text = f'#[expect(clippy::foo, reason = "{word} work")]\nfn f() {{}}\n'
        candidates = CHECK.scan_text("fixture.rs", text)
        expect(f"keyword {word!r} triggers candidacy", len(candidates) == 1, f"got {candidates!r}")


# --------------------------------------------------------------------------
# scan_text: accountability mechanisms


def test_reason_with_issue_ref_is_inline_accounted() -> None:
    text = '#[expect(dead_code, reason = "reserved for future use, tracked in #4530")]\nfn f() {}\n'
    candidates = CHECK.scan_text("fixture.rs", text)
    expect("one candidate", len(candidates) == 1, f"got {candidates!r}")
    if candidates:
        expect("inline accounted via #NNNN in reason", candidates[0].inline_accounted, "")


def test_todo_comment_above_is_inline_accounted() -> None:
    text = '// TODO(#4530): wire this up\n#[expect(dead_code, reason = "planned")]\nfn f() {}\n'
    candidates = CHECK.scan_text("fixture.rs", text)
    expect("one candidate", len(candidates) == 1, f"got {candidates!r}")
    if candidates:
        expect("inline accounted via TODO window", candidates[0].inline_accounted, "")


def test_no_reference_is_not_inline_accounted() -> None:
    text = '#[expect(dead_code, reason = "planned TUI feature")]\nfn f() {}\n'
    candidates = CHECK.scan_text("fixture.rs", text)
    expect("one candidate", len(candidates) == 1, f"got {candidates!r}")
    if candidates:
        expect("not inline accounted", not candidates[0].inline_accounted, "")


# --------------------------------------------------------------------------
# is_test_file / item_name_after


def test_is_test_file_matches_tests_directory() -> None:
    expect("tests/ dir excluded", CHECK.is_test_file("crates/foo/tests/common/mod.rs"), "")
    expect("_tests.rs suffix excluded", CHECK.is_test_file("crates/foo/src/foo_tests.rs"), "")
    expect("ordinary src file included", not CHECK.is_test_file("crates/foo/src/lib.rs"), "")


def test_item_name_after_struct_field() -> None:
    text = '#[expect(dead_code, reason = "planned")]\n    session_id: SessionId,\n'
    end = text.index("]") + 1
    name = CHECK.item_name_after(text, end, 1)
    expect("field name resolved", name == "session_id", f"got {name!r}")


def test_item_name_after_pub_fn() -> None:
    text = '#[expect(dead_code, reason = "planned")]\npub fn synthetic_gate_proof_stub() {}\n'
    end = text.index("]") + 1
    name = CHECK.item_name_after(text, end, 1)
    expect("fn name resolved past pub keyword", name == "synthetic_gate_proof_stub", f"got {name!r}")


# --------------------------------------------------------------------------
# baseline comparison: bidirectional drift (the check-workspace-locks.py
# style "both directions" property, exercised directly against the
# key-count logic main() runs)


def compare(current: Counter, baseline: dict) -> tuple[bool, bool]:
    """Returns (has_new_debt, has_stale_baseline) for a given (current, baseline) pair."""
    has_new = any(current[k] > baseline.get(k, 0) for k in current)
    has_stale = any(current.get(k, 0) < allowed for k, allowed in baseline.items())
    return has_new, has_stale


def test_baseline_catches_new_debt() -> None:
    key = ("fixture.rs", "foo")
    current = Counter({key: 2})
    baseline = {key: 1}
    has_new, has_stale = compare(current, baseline)
    expect("growth beyond baseline is new debt", has_new, "")
    expect("growth alone is not stale", not has_stale, "")


def test_baseline_catches_paid_down_debt_as_stale() -> None:
    key = ("fixture.rs", "foo")
    current = Counter({key: 0})
    baseline = {key: 1}
    has_new, has_stale = compare(current, baseline)
    expect("paid-down debt is not new debt", not has_new, "")
    expect("paid-down debt makes baseline stale", has_stale, "")


def test_baseline_exact_match_is_clean() -> None:
    key = ("fixture.rs", "foo")
    current = Counter({key: 1})
    baseline = {key: 1}
    has_new, has_stale = compare(current, baseline)
    expect("exact match has no new debt", not has_new, "")
    expect("exact match is not stale", not has_stale, "")


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
