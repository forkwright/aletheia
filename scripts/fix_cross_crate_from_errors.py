#!/usr/bin/env python3
"""Parse cargo build errors and revert pub(crate) items that are cross-crate API.

Collects 'is defined here' notes from cargo output and reverts those locations
back to pub.
"""
import re
import subprocess
from pathlib import Path

WORKTREE = Path("/home/ck/aletheia/worktrees/fix/lint-batch1")

# Strip ANSI escape codes
ANSI_RE = re.compile(r"\x1b\[[0-9;]*m")

# Matches lines like:  --> crates/foo/src/bar.rs:42:10
LOCATION_RE = re.compile(r"-->\s+(.+\.rs):(\d+):(\d+)")

# Pattern for pub(crate) items at start of line (with optional indentation)
REVERT_RE = re.compile(r"^(\s*)pub\(crate\)(\s+(?:async\s+)?(?:fn|struct|enum|trait|const|type|mod|static|use)\s)")


def run_cargo_build() -> str:
    result = subprocess.run(
        ["cargo", "build", "--workspace"],
        capture_output=True,
        text=True,
        cwd=str(WORKTREE),
    )
    return ANSI_RE.sub("", result.stdout + result.stderr)


def collect_defined_here_locations(output: str) -> dict[str, set[int]]:
    """Find 'defined here' lines and return {file: {line_numbers}}."""
    locations: dict[str, set[int]] = {}
    lines = output.splitlines()
    for i, line in enumerate(lines):
        # Look for "note: the X Y is defined here" or "note: consider marking X as pub"
        if ("is defined here" in line or "consider marking" in line):
            # The next lines should have --> path:line:col
            for j in range(i + 1, min(i + 4, len(lines))):
                m = LOCATION_RE.search(lines[j])
                if m:
                    path = m.group(1)
                    lineno = int(m.group(2))
                    # Make path absolute
                    if not path.startswith("/"):
                        path = str(WORKTREE / path)
                    locations.setdefault(path, set()).add(lineno)
                    break

    # Also collect E0365 / E0364 errors directly (they point to the re-export site,
    # but the item is defined in a sub-module; we need to find the definition)
    # These are harder to auto-fix, handle separately
    return locations


def revert_line(line: str) -> tuple[str, bool]:
    """Attempt to revert pub(crate) to pub on this line. Returns (new_line, changed)."""
    m = REVERT_RE.match(line)
    if m:
        indent = m.group(1)
        rest = line[len(indent) + len("pub(crate)"):]
        return f"{indent}pub{rest}", True
    # Also handle struct fields: pub(crate) field_name: Type
    if re.match(r"^\s+pub\(crate\)\s+\w", line):
        new = line.replace("pub(crate)", "pub", 1)
        return new, True
    return line, False


def fix_files(locations: dict[str, set[int]]) -> int:
    total = 0
    for path_str, line_numbers in sorted(locations.items()):
        path = Path(path_str)
        if not path.exists():
            print(f"  SKIP (not found): {path_str}")
            continue

        text = path.read_text(encoding="utf-8")
        file_lines = text.splitlines(keepends=True)
        changed_count = 0

        for idx, line in enumerate(file_lines):
            if idx + 1 not in line_numbers:
                continue
            new_line, changed = revert_line(line)
            if changed:
                file_lines[idx] = new_line
                changed_count += 1
            else:
                # Try ±2 lines (sometimes the note points to nearby line)
                for delta in (-1, 1, -2, 2):
                    alt_idx = idx + delta
                    if 0 <= alt_idx < len(file_lines):
                        new_line, changed = revert_line(file_lines[alt_idx])
                        if changed:
                            file_lines[alt_idx] = new_line
                            changed_count += 1
                            break
                if not changed:
                    rel = Path(path_str).relative_to(WORKTREE) if path_str.startswith(str(WORKTREE)) else path_str
                    print(f"  WARN: could not revert line {idx+1} in {rel}: {line.rstrip()}")

        if changed_count:
            path.write_text("".join(file_lines), encoding="utf-8")
            rel = Path(path_str).relative_to(WORKTREE) if path_str.startswith(str(WORKTREE)) else path_str
            print(f"  {rel}: reverted {changed_count} items")
        total += changed_count

    return total


def main() -> None:
    iteration = 0
    while True:
        iteration += 1
        print(f"\n=== Build iteration {iteration} ===")
        output = run_cargo_build()

        # Check for errors
        error_lines = [l for l in output.splitlines() if l.startswith("error[")]
        if not error_lines:
            print("Build succeeded!")
            break

        print(f"Found {len(error_lines)} errors")

        locations = collect_defined_here_locations(output)
        if not locations:
            print("No 'defined here' locations found. Remaining errors may need manual fixing.")
            # Print remaining errors
            for line in output.splitlines():
                if line.startswith("error[") or line.strip().startswith("-->"):
                    print(f"  {line}")
            break

        changed = fix_files(locations)
        if changed == 0:
            print("No changes made despite errors. Manual intervention needed.")
            for line in output.splitlines():
                if line.startswith("error["):
                    print(f"  {line}")
            break

        print(f"  Total reverted: {changed}")

        if iteration >= 10:
            print("Too many iterations, stopping")
            break


if __name__ == "__main__":
    main()
