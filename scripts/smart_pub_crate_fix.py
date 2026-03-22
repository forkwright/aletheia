#!/usr/bin/env python3
"""Smart pub(crate) fixer: applies changes per-item, not per-file.

Unlike the bulk scripts, this script:
1. Applies pub(crate) only to kanon-flagged lines
2. When compilation errors occur, reverts only the SPECIFIC items causing errors
3. Never reverts entire files (avoids regression)
"""
import re
import subprocess
import sys
from pathlib import Path

WORKTREE = Path("/home/ck/aletheia/worktrees/fix/lint-batch1")
ANSI_RE = re.compile(r"\x1b\[[0-9;]*m")
IGNORE_COMMENT = " // kanon:ignore RUST/pub-visibility"


def run(cmd: list[str], timeout: int = 300) -> str:
    result = subprocess.run(cmd, capture_output=True, text=True, cwd=str(WORKTREE), timeout=timeout)
    return ANSI_RE.sub("", result.stdout + result.stderr)


def get_violations() -> dict[str, list[int]]:
    """Run kanon lint and return {file_path: [line_numbers]}."""
    output = run(["kanon", "lint", "--rust", "."])
    violations: dict[str, list[int]] = {}
    for line in output.splitlines():
        if "[RUST/pub-visibility]" not in line:
            continue
        m = re.match(r"(.+\.rs):(\d+)", line)
        if m:
            path, lineno = m.group(1), int(m.group(2))
            violations.setdefault(path, []).append(lineno)
    return violations


def apply_pub_crate(violations: dict[str, list[int]]) -> int:
    """Apply pub(crate) to all flagged lines. Returns count of changes."""
    total = 0
    for path_str, line_numbers in violations.items():
        path = Path(path_str)
        if not path.exists():
            continue
        lines = path.read_text().splitlines(keepends=True)
        changed = 0
        for lineno in sorted(line_numbers):
            idx = lineno - 1
            if idx >= len(lines):
                continue
            line = lines[idx]
            if "kanon:ignore" in line:
                continue
            # Match pub keyword at the declaration level
            m = re.match(r"^(\s*)pub(\s+(?:async\s+)?(?:fn|struct|enum|trait|const|type|mod|static|use|impl|unsafe\s+fn)\b)", line)
            if m:
                lines[idx] = f"{m.group(1)}pub(crate){line[len(m.group(1))+3:]}"
                changed += 1
                continue
            # struct fields: pub field_name: Type
            if re.match(r"^\s+pub\s+\w", line) and "pub(crate)" not in line:
                lines[idx] = line.replace("pub ", "pub(crate) ", 1)
                changed += 1
        if changed:
            path.write_text("".join(lines))
            total += changed
    return total


def get_error_items(output: str) -> list[tuple[str, int, str]]:
    """Parse compilation errors and extract (file_path, line_no, error_code)."""
    items = []
    lines = output.splitlines()
    for i, line in enumerate(lines):
        if not line.startswith("error["):
            continue
        code = re.search(r"\[E(\d+)\]", line)
        if not code:
            continue
        error_code = f"E{code.group(1)}"
        # Find the --> location
        for j in range(i + 1, min(i + 8, len(lines))):
            m = re.search(r"-->\s+(.+\.rs):(\d+):", lines[j])
            if m:
                path = m.group(1)
                if not path.startswith("/"):
                    path = str(WORKTREE / path)
                items.append((path, int(m.group(2)), error_code))
                break
    return items


def revert_item_at(path_str: str, lineno: int) -> bool:
    """Revert pub(crate) to pub at the given line (and nearby). Returns True if changed."""
    path = Path(path_str)
    if not path.exists():
        return False
    lines = path.read_text().splitlines(keepends=True)

    # Try the target line and ±5 lines
    for delta in range(0, 10):
        for sign in (0, -delta, +delta) if delta > 0 else [0]:
            idx = (lineno - 1) + sign
            if not (0 <= idx < len(lines)):
                continue
            line = lines[idx]
            if "pub(crate)" in line:
                lines[idx] = line.replace("pub(crate)", "pub", 1)
                path.write_text("".join(lines))
                return True
    return False


def find_and_revert_item_by_name(output: str, error_line: str) -> bool:
    """For E0365/E0364 errors, find the item by name and revert it."""
    # Extract item name from error like: `ExtractionEngine` is only public...
    m = re.search(r"`(\w+)` is only public within the crate", error_line)
    if not m:
        # Also handle: "is only public within the crate, and cannot be re-exported"
        m = re.search(r"`(\w+)`", error_line)
    if not m:
        return False

    item_name = m.group(1)

    # Search for pub(crate) def of this item in the entire crate
    search = subprocess.run(
        ["grep", "-rn", f"pub(crate).*{item_name}", "crates/"],
        capture_output=True, text=True, cwd=str(WORKTREE)
    )

    changed = False
    for line in search.stdout.splitlines():
        m2 = re.match(r"(.+\.rs):(\d+):(.*pub\(crate\))", line)
        if not m2:
            continue
        path_str = str(WORKTREE / m2.group(1))
        lineno = int(m2.group(2))
        content = m2.group(3)
        # Verify the item name appears in the definition
        if item_name not in content:
            continue
        if revert_item_at(path_str, lineno):
            print(f"    reverted {item_name} at {m2.group(1)}:{lineno}")
            changed = True
    return changed


def main():
    print("Getting current violations...")
    violations = get_violations()
    total_violations = sum(len(v) for v in violations.items())
    print(f"Found {sum(len(v) for v in violations.values())} violations across {len(violations)} files")

    print("Applying pub(crate)...")
    changed = apply_pub_crate(violations)
    print(f"Applied pub(crate) to {changed} items")

    # Iterative fix loop
    for iteration in range(1, 20):
        print(f"\n=== Iteration {iteration} ===")
        output = run(["cargo", "build", "--workspace"], timeout=300)
        error_lines = [l for l in output.splitlines() if l.startswith("error[")]

        if not error_lines:
            print("Build succeeded!")
            break

        print(f"{len(error_lines)} errors")

        # Get error items with locations
        error_items = get_error_items(output)

        # Process E0624 errors first (they point to call sites, fix via search)
        # Process E0365/E0446 errors (they need name-based search)
        reverted = 0

        # For each unique error, try to fix
        seen_errors: set[str] = set()
        for error_line in error_lines:
            error_key = error_line.strip()
            if error_key in seen_errors:
                continue
            seen_errors.add(error_key)

            code_m = re.search(r"\[E(\d+)\]", error_line)
            if not code_m:
                continue
            code = f"E{code_m.group(1)}"

            if code in ("E0365", "E0364", "E0446"):
                if find_and_revert_item_by_name(output, error_line):
                    reverted += 1

        # For E0624 errors, use the "defined here" approach from the other script
        lines = output.splitlines()
        for i, line in enumerate(lines):
            if ("is defined here" in line or "consider marking" in line):
                for j in range(i + 1, min(i + 4, len(lines))):
                    m = re.search(r"-->\s+(.+\.rs):(\d+):", lines[j])
                    if m:
                        path = m.group(1)
                        if not path.startswith("/"):
                            path = str(WORKTREE / path)
                        lineno = int(m.group(2))
                        if revert_item_at(path, lineno):
                            rel = path.replace(str(WORKTREE) + "/", "")
                            print(f"    reverted at {rel}:{lineno}")
                            reverted += 1
                        break

        if reverted == 0:
            print("No items reverted - manual intervention needed")
            # Show sample errors
            for el in error_lines[:5]:
                print(f"  {el}")
            break

        print(f"Reverted {reverted} items")

    # Count final violations
    output = run(["kanon", "lint", "--rust", "."])
    final_count = sum(1 for l in output.splitlines() if "[RUST/pub-visibility]" in l)
    print(f"\nFinal violation count: {final_count}")
    reduction = (1208 - final_count) / 1208 * 100
    print(f"Reduction from 1208: {reduction:.1f}%")


if __name__ == "__main__":
    main()
