#!/usr/bin/env python3
"""Tests for check-pr-closes-keyword.py."""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

SPEC = importlib.util.spec_from_file_location(
    "check_pr_closes_keyword",
    Path(__file__).resolve().parent / "check-pr-closes-keyword.py",
)
assert SPEC and SPEC.loader
CHECK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECK)


# Bodies that MUST be rejected. The first three are verbatim from merged PRs
# that stranded real issues (#6717, #6718, #6661).
REJECT = [
    "Closes #5173, #5355, #5368, #5605, #6666",
    "Closes #5177, #6507",
    "closes #4638, #5646, #5271",
    "Fixes #1, #2",
    "resolves #10,#11",
    "Some prose first.\n\nCloses #900, #901\n",
]

# Bodies that MUST pass.
ACCEPT = [
    "Closes #5128",
    "Closes #5128\nCloses #5349",
    # Correct as written: one issue closed, another merely referenced.
    "Closes #123, and relates to work in #456",
    "See #1, #2 for context",
    "Related: #10, #11",
    "No issue references at all.",
    # A body documenting the rule has to quote the bad form.
    "Do not write `Closes #1, #2` — repeat the keyword instead.",
    "Example:\n\n```\nCloses #1, #2\n```\n\nUse one per line.",
]


def main() -> int:
    failures: list[str] = []

    for body in REJECT:
        if not CHECK.violations(body):
            failures.append(f"should have been REJECTED: {body!r}")

    for body in ACCEPT:
        found = CHECK.violations(body)
        if found:
            failures.append(f"should have been ACCEPTED: {body!r} (matched {found})")

    # A violation must report the line it is actually on.
    numbered = CHECK.violations("intro\n\nCloses #7, #8\ntail")
    if numbered != [(3, "Closes #7, #8")]:
        failures.append(f"wrong line attribution: {numbered}")

    # Blanking must not shift line numbers.
    spanned = CHECK.violations("`Closes #1, #2`\nCloses #3, #4")
    if spanned != [(2, "Closes #3, #4")]:
        failures.append(f"code-span blanking shifted attribution: {spanned}")

    # A closing keyword inside a sentence closes the issue anyway (#4719, #6908).
    # Both bodies below are the real ones that did it.
    prose_reject = [
        "## This does not close #4719",
        "I was about to\nclose #6908 on it.",
        "I was about to close #6908 on it.",
        "This PR closes #55 and adds tests.",
        "The change here\nfixes #77 as a side effect.",
    ]
    prose_accept = [
        "Closes #123",
        "- Closes #123",
        "1. Closes #123",
        "Some prose.\n\nCloses #123",
        "Refs #6908",
        "Related: #123 covers the other half.",
        "Write `Closes #123` at the start of a line.",
        "```\nCloses #123\n```",
    ]
    for body in prose_reject:
        if not CHECK.prose_keywords(body):
            failures.append(f"prose keyword should have been REJECTED: {body!r}")
    for body in prose_accept:
        found = CHECK.prose_keywords(body)
        if found:
            failures.append(f"prose keyword should have been ACCEPTED: {body!r} ({found})")

    # WHY the wrapped case matters most: a hard-wrapped body puts arbitrary
    # words at a line start, so a line-based rule reads "close #6908 on it" as
    # deliberate. It is the second line of "I was about to close #6908 on it",
    # and it is exactly how #6908 was closed.
    wrapped = CHECK.prose_keywords("I was about to\nclose #6908 on it.")
    if wrapped != [(2, "close #6908 on it.")]:
        failures.append(f"wrapped prose keyword attribution: {wrapped}")

    if failures:
        for failure in failures:
            print(f"FAIL: {failure}", file=sys.stderr)
        return 1

    print(
        f"OK: {len(REJECT)} rejected, {len(ACCEPT)} accepted, "
        f"{len(prose_reject)} prose rejected, {len(prose_accept)} prose accepted, "
        "attribution correct"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
