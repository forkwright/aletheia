#!/usr/bin/env python3
"""Every ZIP we emit must be byte-reproducible, so no entry may carry a wall clock.

`zip::write::SimpleFileOptions::default()` sets `last_modified_time` to
`DateTime::default_for_write()`, which is `OffsetDateTime::now_utc()` whenever the zip
crate's `time` feature is enabled -- and it is, transitively. Every archive therefore
stamps the current time into each local file header, at bytes 10-13.

Two emissions of byte-identical content are then byte-identical only while they land in
the same second. That is a real non-determinism wearing a flaky test's costume: it
passes almost always, and every failure is a true positive nobody can reproduce.

It took down the release. `poiesis-theme sinks::pptx::tests::pptx_byte_stable_across_runs`
asserts "two emissions must match byte-for-byte", and on the 0.40.0 release head it
failed -- taking `gate` with it, on a release branch that had finally got its checks
running. The diff was the ZIP local header: `50 4b 03 04 | 14 00 | 00 00 | 08 00 | <mod
time> | <mod date>`.

The fix at each site is `.last_modified_time(zip::DateTime::DEFAULT)` -- the ZIP epoch,
1980-01-01, which is the reproducible-build convention. This check refuses a bare
`SimpleFileOptions::default()` so the next sink cannot reintroduce it silently.
"""

from __future__ import annotations

import logging
import re
import subprocess
import sys
from pathlib import Path

LOGGER = logging.getLogger("check-zip-determinism")

REPO_ROOT = Path(__file__).resolve().parents[1]

# A construction that is NOT immediately followed by an explicit timestamp.
# `.last_modified_time(...)` may appear anywhere in the builder chain, so the check is
# per-construction rather than per-line: the chain can wrap across lines.
CONSTRUCTION = re.compile(r"SimpleFileOptions::default\(\)")
FIXED_TIME = "last_modified_time"


def tracked_rust_files(repo_root: Path) -> list[Path]:
    result = subprocess.run(
        ["git", "ls-files", "*.rs"],
        capture_output=True, text=True, check=False, cwd=repo_root,
    )
    if result.returncode != 0:
        raise SystemExit(f"git ls-files failed: {result.stderr.strip()}")
    return [repo_root / line for line in result.stdout.splitlines() if line]


def unstamped_constructions(text: str) -> list[tuple[int, str]]:
    """Each `SimpleFileOptions::default()` whose builder chain sets no timestamp.

    WHY the chain and not the line: these are written both as a one-liner and as a
    multi-line builder. Checking only the matching line would pass a construction whose
    `.compression_method(...)` pushed the rest onto the next line.
    """
    lines = text.split("\n")
    findings = []
    for index, line in enumerate(lines):
        if not CONSTRUCTION.search(line):
            continue
        # The construction plus the rest of its statement: everything up to the first
        # `;` at or after this line, bounded so a missing semicolon cannot run away.
        chain = []
        for cursor in range(index, min(index + 8, len(lines))):
            chain.append(lines[cursor])
            if ";" in lines[cursor]:
                break
        if FIXED_TIME not in "\n".join(chain):
            findings.append((index + 1, line.strip()))
    return findings


def main() -> int:
    failures: list[str] = []
    seen = 0

    for path in tracked_rust_files(REPO_ROOT):
        try:
            text = path.read_text(encoding="utf-8")
        except (OSError, UnicodeError):
            continue
        if "SimpleFileOptions" not in text:
            continue
        seen += len(CONSTRUCTION.findall(text))
        for lineno, line in unstamped_constructions(text):
            failures.append(f"  {path.relative_to(REPO_ROOT).as_posix()}:{lineno}  {line}")

    if failures:
        LOGGER.error(
            "check-zip-determinism: a ZIP entry is being written with the current time."
        )
        for line in failures:
            LOGGER.error("%s", line)
        LOGGER.error("")
        LOGGER.error("`SimpleFileOptions::default()` stamps OffsetDateTime::now_utc()")
        LOGGER.error("into the local file header, so two emissions of identical content")
        LOGGER.error("differ in bytes 10-13 as soon as they straddle a second boundary.")
        LOGGER.error("")
        LOGGER.error("Add `.last_modified_time(zip::DateTime::DEFAULT)` -- the ZIP epoch,")
        LOGGER.error("which is the reproducible-build convention. This exact defect")
        LOGGER.error("failed `pptx_byte_stable_across_runs` and reddened the 0.40.0")
        LOGGER.error("release gate, and it reproduces roughly one run in however many")
        LOGGER.error("cross a second boundary -- which is why it survived this long.")
        return 1

    LOGGER.info(
        "check-zip-determinism: %d SimpleFileOptions construction(s), all with an "
        "explicit timestamp",
        seen,
    )
    return 0


if __name__ == "__main__":
    logging.basicConfig(format="%(message)s", level=logging.INFO, stream=sys.stderr)
    raise SystemExit(main())
