#!/usr/bin/env python3
"""Revert pub(crate) back to pub for items that are part of cross-crate API.

These items caused compilation errors after the bulk pub-visibility fix
because they are genuinely used by other crates.
"""
import re
from pathlib import Path

WORKTREE = Path("/home/ck/aletheia/worktrees/fix/lint-batch1")

# (file_relative, line_number, item_name_hint)
# line_number is 1-based, approximate - we search within ±5 lines
REVERSIONS = [
    # koina/src/secret.rs - SecretString is cross-crate public API
    ("crates/koina/src/secret.rs", 18, "SecretString"),
    # koina/src/credential.rs - Credential and CredentialProvider are cross-crate
    ("crates/koina/src/credential.rs", 30, "Credential"),
    ("crates/koina/src/credential.rs", 44, "CredentialProvider"),
    # koina/src/cleanup.rs - CleanupRegistry is cross-crate
    ("crates/koina/src/cleanup.rs", 89, "CleanupRegistry"),
    # koina/src/http.rs - CONTENT_TYPE_EVENT_STREAM is cross-crate
    ("crates/koina/src/http.rs", 7, "CONTENT_TYPE_EVENT_STREAM"),
    # koina/src/system.rs - FileSystem and Environment traits are cross-crate
    ("crates/koina/src/system.rs", 54, "FileSystem"),
    ("crates/koina/src/system.rs", 141, "Environment"),
    # koina/src/defaults.rs - constants used by taxis
    ("crates/koina/src/defaults.rs", 6, "MAX_OUTPUT_TOKENS"),
    ("crates/koina/src/defaults.rs", 9, "BOOTSTRAP_MAX_TOKENS"),
    ("crates/koina/src/defaults.rs", 12, "CONTEXT_TOKENS"),
    ("crates/koina/src/defaults.rs", 15, "MAX_TOOL_ITERATIONS"),
    ("crates/koina/src/defaults.rs", 18, "MAX_TOOL_RESULT_BYTES"),
    ("crates/koina/src/defaults.rs", 24, "HISTORY_BUDGET_RATIO"),
    ("crates/koina/src/defaults.rs", 27, "CHARS_PER_TOKEN"),
    # koina/src/disk_space.rs - available_space used by taxis
    ("crates/koina/src/disk_space.rs", 72, "available_space"),
    # eidos/src/id.rs - IdValidationError appears in TryFrom impls (public interface)
    ("crates/eidos/src/id.rs", 103, "IdValidationError"),
    # daemon maintenance structs re-exported by daemon
    ("crates/daemon/src/maintenance/db_monitor.rs", 1, "DbMonitor"),
    ("crates/daemon/src/maintenance/drift_detection.rs", 1, "DriftDetector"),
    ("crates/daemon/src/maintenance/knowledge.rs", 1, "KnowledgeMaintenanceExecutor"),
    ("crates/daemon/src/maintenance/retention.rs", 1, "RetentionExecutor"),
    ("crates/daemon/src/maintenance/trace_rotation.rs", 1, "TraceRotator"),
    ("crates/daemon/src/maintenance/trace_rotation.rs", 1, "allow_non_essential_write"),
    ("crates/daemon/src/maintenance/trace_rotation.rs", 1, "status"),
]

# Pattern: matches pub(crate) followed by various item kinds
CRATE_PUB_RE = re.compile(r"(\s*)pub\(crate\)(\s+(?:async\s+)?(?:fn|struct|enum|trait|const|type|mod|use|static)\s+\w)")


def fix_file_items(path: Path, item_names: list[str]) -> int:
    """Revert pub(crate) to pub for items matching the given names in a file."""
    if not path.exists():
        print(f"  SKIP (not found): {path}")
        return 0

    text = path.read_text(encoding="utf-8")
    lines = text.splitlines(keepends=True)
    changed = 0

    for idx, line in enumerate(lines):
        if "pub(crate)" not in line:
            continue
        # Check if this line defines one of our target items
        for name in item_names:
            # Match: pub(crate) [async] fn/struct/enum/trait/const name
            # or pub(crate) struct/enum Name { (struct fields)
            if re.search(rf"\bpub\(crate\)\s+(?:async\s+)?(?:fn|struct|enum|trait|const|type|mod|static)\s+{re.escape(name)}\b", line):
                new_line = line.replace("pub(crate)", "pub", 1)
                lines[idx] = new_line
                changed += 1
                break

    if changed:
        path.write_text("".join(lines), encoding="utf-8")
    return changed


def main() -> None:
    # Group reversions by file
    by_file: dict[str, list[str]] = {}
    for rel_path, _lineno, name in REVERSIONS:
        by_file.setdefault(rel_path, []).append(name)

    total = 0
    for rel_path, names in sorted(by_file.items()):
        path = WORKTREE / rel_path
        changed = fix_file_items(path, names)
        if changed:
            print(f"  {rel_path}: reverted {changed} items ({', '.join(names[:changed])})")
        else:
            # Try a broader search - item might be on a different line
            print(f"  {rel_path}: searching for {names}...")
            text = path.read_text(encoding="utf-8") if path.exists() else ""
            for name in names:
                if f"pub(crate)" in text and name in text:
                    print(f"    WARNING: {name} found but pattern didn't match - check manually")
        total += changed

    print(f"\nTotal reverted: {total}")


if __name__ == "__main__":
    main()
