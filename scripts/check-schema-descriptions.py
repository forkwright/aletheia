#!/usr/bin/env python3
"""A doc comment on a `JsonSchema` type is published API, not a maintainer note.

`schemars` projects a `///` verbatim into the generated schema's `description`, so
every doc comment on a schema-bearing type or field is read by whoever consumes the
schema -- an MCP client, a generated SDK, a docs page.

WHY this guard exists, given the tree is currently clean: two rules in this repository
point in opposite directions here, and nothing mediates between them.

  * `STANDARDS.md` requires comments to use structured tags -- WHY, WARNING, SAFETY,
    INVARIANT, SECURITY, TODO(#NNNN) and the rest -- and forbids freeform prose.
  * Those same tags are maintainer notes. Projected into a published description they
    are noise at best, and at worst they narrate internal reasoning, a threat model, or
    an unfixed defect to an external consumer.

So a maintainer writing `/// WHY: we clamp this because the store panics above 10k`
on a schema field is *following the comment standard* and leaking at the same time.
Nothing about the two rules reveals the conflict at the point of writing.

The relayed case that prompted this: a paragraph explaining crate layout reached three
published schemas in a sibling repository, and the drift check there caught the bytes
CHANGING while nothing said the new bytes were WRONG.

Aletheia has 28 `JsonSchema`-deriving types and zero such tags today. That is a property
held by discipline rather than by construction, which is exactly the kind worth pinning
before it lapses -- a baseline of zero is the cheapest moment to start.
"""

from __future__ import annotations

import logging
import re
import subprocess
import sys
from pathlib import Path

LOGGER = logging.getLogger("check-schema-descriptions")

REPO_ROOT = Path(__file__).resolve().parents[1]

DERIVE_JSON_SCHEMA = re.compile(r"#\[derive\([^)]*\bJsonSchema\b")

# The structured-tag vocabulary STANDARDS.md reserves for maintainer notes. Matched as
# whole words so an ordinary sentence mentioning "note" or "why" is not a finding.
MAINTAINER_TAGS = (
    "WHY", "WARNING", "NOTE", "PERF", "SAFETY", "INVARIANT", "SECURITY",
    "TODO", "FIXME", "HACK", "XXX",
)
TAG = re.compile(rf"\b({'|'.join(MAINTAINER_TAGS)})\b")

DOC_COMMENT = re.compile(r"^\s*///")


def tracked_rust_files(repo_root: Path) -> list[Path]:
    result = subprocess.run(
        ["git", "ls-files", "*.rs"],
        capture_output=True, text=True, check=False, cwd=repo_root,
    )
    if result.returncode != 0:
        raise SystemExit(f"git ls-files failed: {result.stderr.strip()}")
    return [repo_root / line for line in result.stdout.splitlines() if line]


def leaking_docs(text: str) -> list[tuple[int, str]]:
    """Doc comments carrying a maintainer tag inside a `JsonSchema` item.

    Covers the doc block ABOVE the derive -- which becomes the schema's own
    `description` -- and every field doc INSIDE the item, which become property
    descriptions. Both are published; only the second is easy to forget.
    """
    lines = text.split("\n")
    findings: list[tuple[int, str]] = []

    for index, line in enumerate(lines):
        if not DERIVE_JSON_SCHEMA.search(line):
            continue

        # The doc block immediately above the derive, walking back through other
        # attributes -- `#[serde(...)]` commonly sits between the docs and the derive.
        cursor = index - 1
        while cursor >= 0 and (
            DOC_COMMENT.match(lines[cursor]) or lines[cursor].strip().startswith("#[")
        ):
            if DOC_COMMENT.match(lines[cursor]) and TAG.search(lines[cursor]):
                findings.append((cursor + 1, lines[cursor].strip()))
            cursor -= 1

        # The item body, to its closing brace. Tuple structs and unit types end on the
        # same line and simply contribute nothing.
        depth = 0
        opened = False
        for cursor in range(index + 1, len(lines)):
            body = lines[cursor]
            depth += body.count("{") - body.count("}")
            if "{" in body:
                opened = True
            if DOC_COMMENT.match(body) and TAG.search(body):
                findings.append((cursor + 1, body.strip()))
            if opened and depth <= 0:
                break

    return sorted(set(findings))


def main() -> int:
    total = 0
    schema_items = 0
    failures: list[str] = []

    for path in tracked_rust_files(REPO_ROOT):
        try:
            text = path.read_text(encoding="utf-8")
        except (OSError, UnicodeError):
            continue
        if "JsonSchema" not in text:
            continue
        schema_items += len(DERIVE_JSON_SCHEMA.findall(text))
        for lineno, line in leaking_docs(text):
            total += 1
            failures.append(f"  {path.relative_to(REPO_ROOT).as_posix()}:{lineno}  {line}")

    if failures:
        LOGGER.error(
            "check-schema-descriptions: a maintainer note is being published as a "
            "schema description."
        )
        for line in failures:
            LOGGER.error("%s", line)
        LOGGER.error("")
        LOGGER.error("`schemars` projects a `///` verbatim into the generated schema,")
        LOGGER.error("so this text is read by whoever consumes it -- an MCP client, a")
        LOGGER.error("generated SDK, a docs page. A structured tag there narrates")
        LOGGER.error("internal reasoning, or an unfixed defect, to an external reader.")
        LOGGER.error("")
        LOGGER.error("Move the note to a `//` comment inside the impl or above the")
        LOGGER.error("attribute block, and leave the `///` saying what the field IS.")
        return 1

    LOGGER.info(
        "check-schema-descriptions: %d JsonSchema item(s), no maintainer tags in any "
        "published description",
        schema_items,
    )
    return 0


if __name__ == "__main__":
    logging.basicConfig(format="%(message)s", level=logging.INFO, stream=sys.stderr)
    raise SystemExit(main())
