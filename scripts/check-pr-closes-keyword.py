#!/usr/bin/env python3
"""Reject a closing keyword followed by a comma-separated issue list."""

from __future__ import annotations

import logging
import os
import re
import sys

LOGGER = logging.getLogger("check-pr-closes-keyword")

# WHY the pattern demands a second `#N` after the comma: `Closes #123, and see
# #456` is correct as written -- one issue closed, another merely referenced,
# which is exactly what GitHub does. Only a comma directly joining two issue
# references expresses an intent GitHub will not honour.
VIOLATION = re.compile(
    r"\b(?:clos(?:e|es|ed)|fix(?:es|ed)?|resolve[sd]?)\s+#\d+\s*,\s*#\d+",
    re.IGNORECASE,
)

FENCED = re.compile(r"```.*?```", re.DOTALL)
INLINE = re.compile(r"`[^`\n]*`")


def blank_code_spans(body: str) -> str:
    """Replace code content with spaces, preserving offsets and line breaks.

    WHY: a body documenting this very rule has to quote the bad form, and a
    keyword inside code formatting is an example rather than an instruction --
    GitHub does not act on it either. Offsets are preserved rather than the
    spans deleted so reported line numbers still point at the real body.
    """

    def blank(match: re.Match[str]) -> str:
        return "".join("\n" if char == "\n" else " " for char in match.group(0))

    return INLINE.sub(blank, FENCED.sub(blank, body))


def violations(body: str) -> list[tuple[int, str]]:
    """Return `(line number, line)` for each comma-listed closing keyword."""
    scannable = blank_code_spans(body)
    found: list[tuple[int, str]] = []
    original = body.splitlines()
    for index, line in enumerate(scannable.splitlines(), start=1):
        if VIOLATION.search(line):
            found.append((index, original[index - 1].strip()))
    return found


def main() -> int:
    body = os.environ.get("PR_BODY")
    if body is None:
        body = sys.stdin.read()

    found = violations(body)
    if not found:
        LOGGER.info("pr-closes-keyword: clean")
        return 0

    LOGGER.error(
        "pr-closes-keyword: a closing keyword is followed by a comma-separated list."
    )
    for line_number, line in found:
        LOGGER.error("  line %d: %s", line_number, line)
    LOGGER.error("")
    LOGGER.error(
        "GitHub closes ONLY the first issue in such a list. The rest merge with"
    )
    LOGGER.error("their work done and stay open.")
    LOGGER.error("")
    LOGGER.error("Repeat the keyword per issue, one per line:")
    LOGGER.error("  Closes #123")
    LOGGER.error("  Closes #456")
    return 1


if __name__ == "__main__":
    logging.basicConfig(format="%(message)s", level=logging.INFO, stream=sys.stderr)
    raise SystemExit(main())
