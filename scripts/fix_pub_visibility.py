#!/usr/bin/env python3
"""Fix RUST/pub-visibility violations from kanon lint output.

Reads kanon violations from stdin or file, then replaces bare `pub` with
`pub(crate)` at the flagged lines, skipping lines already using restricted
visibility or kanon:ignore markers.
"""
import re
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

WORKTREE = Path("/home/ck/aletheia/worktrees/fix/lint-batch1")

# Pattern: extract file path and line number from kanon output
# Format: /path/to/file.rs:NN [RUST/pub-visibility] ...
VIOLATION_RE = re.compile(r"^(\S+\.rs):(\d+)\s.*\[RUST/pub-visibility\]")

# Matches bare `pub ` at start of identifier (not pub(crate), pub(super), pub(in ...))
PUB_RE = re.compile(r"^(\s*)pub\s+(?!\()")


def get_violations() -> dict[str, list[int]]:
    """Run kanon and collect pub-visibility violation locations."""
    result = subprocess.run(
        ["kanon", "lint", "--rust", str(WORKTREE)],
        capture_output=True,
        text=True,
    )
    # kanon uses ANSI codes; strip them
    ansi_escape = re.compile(r"\x1b\[[0-9;]*m")
    output = ansi_escape.sub("", result.stdout + result.stderr)

    violations: dict[str, list[int]] = defaultdict(list)
    for line in output.splitlines():
        m = VIOLATION_RE.match(line.strip())
        if m:
            path, lineno = m.group(1), int(m.group(2))
            violations[path].append(lineno)
    return violations


def fix_file(path: str, lines_to_fix: list[int]) -> int:
    """Apply pub(crate) fixes to flagged lines in a file. Returns count changed."""
    p = Path(path)
    if not p.exists():
        print(f"  SKIP (not found): {path}", file=sys.stderr)
        return 0

    text = p.read_text(encoding="utf-8")
    file_lines = text.splitlines(keepends=True)
    changed = 0

    line_set = set(lines_to_fix)
    for idx, line in enumerate(file_lines):
        lineno = idx + 1
        if lineno not in line_set:
            continue

        # Skip if already restricted or has ignore marker
        if "pub(crate)" in line or "pub(super)" in line or "pub(in" in line:
            continue
        if "kanon:ignore" in line:
            continue

        m = PUB_RE.match(line)
        if m:
            indent = m.group(1)
            rest = line[len(indent) + 3:]  # skip "pub"
            # rest starts with whitespace before the item keyword
            file_lines[idx] = f"{indent}pub(crate){rest}"
            changed += 1

    if changed:
        p.write_text("".join(file_lines), encoding="utf-8")

    return changed


def main() -> None:
    print("Collecting RUST/pub-visibility violations...")
    violations = get_violations()
    total_files = len(violations)
    total_violations = sum(len(v) for v in violations.values())
    print(f"Found {total_violations} violations across {total_files} files")

    total_changed = 0
    for path, lines in sorted(violations.items()):
        changed = fix_file(path, lines)
        if changed:
            rel = Path(path).relative_to(WORKTREE)
            print(f"  {rel}: {changed}/{len(lines)} fixed")
            total_changed += changed

    print(f"\nTotal changed: {total_changed}/{total_violations}")


if __name__ == "__main__":
    main()
