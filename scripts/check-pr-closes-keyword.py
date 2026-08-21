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

# WHY(#4719, and again on #6908): GitHub's parser matches a closing keyword
# ANYWHERE in the body, including inside a sentence that says the opposite.
# #4719 was closed by a PR whose body carried the heading "## This does not
# close #4719". #6908 was closed by a PR whose body read "I was about to close
# #6908 on it" -- a sentence reporting that it had NOT been closed.
#
# Both are well-formed keywords in prose, so the comma rule above cannot see
# them and no amount of enumerating negations would have caught both ("does
# not", "was about to"). The rule that separates them from the real thing is
# positional rather than semantic: a body that means to close an issue writes
# the keyword at the start of a line, and prose that merely mentions one never
# does. Anything mid-sentence is therefore ambiguous at best and inverted at
# worst, and GitHub acts on it either way.
KEYWORD = r"(?:clos(?:e|es|ed)|fix(?:es|ed)?|resolve[sd]?)"
ANY_KEYWORD = re.compile(rf"\b{KEYWORD}\s+#\d+", re.IGNORECASE)
# WHY the block rather than the line: a hard-wrapped body puts arbitrary words
# at the start of a line. #6908's closing sentence wrapped as "I was about to\n
# close #6908 on it", so a line-start rule reads its second line as a deliberate
# keyword -- and that is the case this check exists for. Intent lives at the
# start of a paragraph or list item, which survives rewrapping.
BLOCK_START = re.compile(rf"^\s*(?:[-*+]\s+|\d+\.\s+)?{KEYWORD}\s+#\d+", re.IGNORECASE)

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


def prose_keywords(body: str) -> list[tuple[int, str]]:
    """Return `(line number, line)` for each closing keyword inside a sentence.

    Blocks are separated by blank lines and by list-item starts, so a keyword
    opening either one is deliberate. A keyword anywhere else in the block is
    prose, whatever line the wrapping happened to put it on.
    """
    scannable = blank_code_spans(body)
    original = body.splitlines()
    found: list[tuple[int, str]] = []

    block: list[tuple[int, str]] = []

    def flush(block: list[tuple[int, str]]) -> None:
        if not block:
            return
        if BLOCK_START.match(block[0][1]):
            return
        joined = " ".join(text for _, text in block)
        if not ANY_KEYWORD.search(joined):
            return
        for index, text in block:
            if ANY_KEYWORD.search(text):
                found.append((index, original[index - 1].strip()))
                return
        found.append((block[0][0], original[block[0][0] - 1].strip()))

    for index, line in enumerate(scannable.splitlines(), start=1):
        starts_item = re.match(r"^\s*(?:[-*+]\s+|\d+\.\s+)", line) is not None
        if not line.strip() or starts_item:
            flush(block)
            block = []
        if line.strip():
            block.append((index, line))
    flush(block)
    return found


def main() -> int:
    body = os.environ.get("PR_BODY")
    if body is None:
        body = sys.stdin.read()

    found = violations(body)
    prose = prose_keywords(body)
    if not found and not prose:
        LOGGER.info("pr-closes-keyword: clean")
        return 0

    if found:
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

    if prose:
        if found:
            LOGGER.error("")
        LOGGER.error(
            "pr-closes-keyword: a closing keyword appears inside a sentence."
        )
        for line_number, line in prose:
            LOGGER.error("  line %d: %s", line_number, line)
        LOGGER.error("")
        LOGGER.error("GitHub acts on the keyword wherever it appears, including in a")
        LOGGER.error("sentence saying the issue is NOT being closed. #4719 was closed")
        LOGGER.error('by a body reading "This does not close #4719"; #6908 by one')
        LOGGER.error('reading "I was about to close #6908 on it".')
        LOGGER.error("")
        LOGGER.error("If you mean to close it, put the keyword at the start of a line:")
        LOGGER.error("  Closes #123")
        LOGGER.error("If you do not, refer to the issue by number alone: #123")
    return 1


if __name__ == "__main__":
    logging.basicConfig(format="%(message)s", level=logging.INFO, stream=sys.stderr)
    raise SystemExit(main())
