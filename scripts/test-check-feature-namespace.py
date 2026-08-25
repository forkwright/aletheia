#!/usr/bin/env python3
"""Tests for check-feature-namespace.py.

Covers the two pure units the checker is built from: feature-name
enumeration from a `--no-deps` metadata document (member filtering,
unioning, `default` exclusion) and failure-detail extraction from cargo's
stderr (ANSI stripping, error-line preference, fallbacks). Mirrors
test-check-stub-accountability.py's harness: no pytest dependency, plain
`expect()` assertions collected into one FAILURES list.
"""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

SPEC = importlib.util.spec_from_file_location(
    "check_feature_namespace",
    Path(__file__).resolve().parent / "check-feature-namespace.py",
)
assert SPEC and SPEC.loader
CHECK = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = CHECK
SPEC.loader.exec_module(CHECK)

FAILURES: list[str] = []


def expect(label: str, cond: bool, detail: str = "") -> None:
    if not cond:
        FAILURES.append(f"{label}: {detail}" if detail else label)


METADATA = {
    "workspace_members": ["a", "b"],
    "packages": [
        {
            "id": "a",
            "name": "aaa",
            "features": {"default": [], "fjall-helpers": [], "test-core": []},
        },
        {
            "id": "b",
            "name": "bbb",
            "features": {"default": ["store"], "store": []},
        },
        {
            "id": "c",
            "name": "external-dep",
            "features": {"outside": []},
        },
    ],
}


def test_feature_names_union_sorted_without_default() -> None:
    names = CHECK.workspace_feature_names(METADATA)
    expect(
        "names are the sorted member union minus default",
        names == ["fjall-helpers", "store", "test-core"],
        f"got {names!r}",
    )


def test_feature_names_exclude_non_members() -> None:
    names = CHECK.workspace_feature_names(METADATA)
    expect("non-member feature excluded", "outside" not in names, f"got {names!r}")


def test_feature_names_empty_workspace() -> None:
    expect(
        "no members yields no names",
        CHECK.workspace_feature_names({"workspace_members": [], "packages": []}) == [],
        "",
    )


def test_failure_detail_prefers_error_line() -> None:
    stderr = (
        "    Blocking waiting for file lock on package cache\n"
        "error: package `episteme v0.42.1 (/project/crates/episteme)` "
        "does not have feature `fjall`\n"
        "\n"
        "help: an optional dependency with that name exists\n"
    )
    detail = CHECK.failure_detail(stderr)
    expect(
        "error line returned",
        detail == "error: package `episteme v0.42.1 (/project/crates/episteme)` "
        "does not have feature `fjall`",
        f"got {detail!r}",
    )


def test_failure_detail_strips_ansi() -> None:
    stderr = "\x1b[1m\x1b[91merror\x1b[0m: package `x` does not have feature `y`\n"
    detail = CHECK.failure_detail(stderr)
    expect("ansi codes stripped", detail == "error: package `x` does not have feature `y`", f"got {detail!r}")


def test_failure_detail_falls_back_to_first_nonempty_line() -> None:
    stderr = "\n   \nwarning: something odd happened\n"
    detail = CHECK.failure_detail(stderr)
    expect(
        "non-error fallback",
        detail == "warning: something odd happened",
        f"got {detail!r}",
    )


def test_failure_detail_empty_stderr() -> None:
    expect(
        "empty stderr is explicit",
        CHECK.failure_detail("") == "(cargo metadata failed with no stderr output)",
        "",
    )


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
