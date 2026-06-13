#!/usr/bin/env python3
"""Check public docs for onboarding and runtime-environment contract drift."""

from __future__ import annotations

import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
FAILURES: list[str] = []


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def require_contains(path: str, needle: str, reason: str) -> None:
    if needle not in read(path):
        FAILURES.append(f"{path}: missing {reason}: {needle!r}")


def require_absent(path: str, needle: str, reason: str) -> None:
    if needle in read(path):
        FAILURES.append(f"{path}: forbidden {reason}: {needle!r}")


def markdown_files() -> list[Path]:
    roots = [ROOT / "README.md", *sorted((ROOT / "docs").rglob("*.md"))]
    roots.extend(sorted((ROOT / "instance.example").rglob("*.md")))
    return [path for path in roots if path.is_file()]


def check_public_snippets() -> None:
    local_path = re.compile(
        r"(?<![\w/])"
        r"("
        r"/data/(?:target|wt|worktrees)[^\s`\"'<>)]*"
        r"|/home/ck[^\s`\"'<>)]*"
        r"|/Users/ck[^\s`\"'<>)]*"
        r")"
    )
    for path in markdown_files():
        rel = path.relative_to(ROOT)
        in_fence = False
        fence_start = 0
        for lineno, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
            if line.lstrip().startswith("```"):
                in_fence = not in_fence
                fence_start = lineno if in_fence else 0
                continue
            if in_fence and (match := local_path.search(line)):
                FAILURES.append(
                    f"{rel}:{lineno}: maintainer-local path in public snippet "
                    f"(fence starts at line {fence_start}): {match.group(1)}"
                )


def check_env_contract() -> None:
    require_contains(
        ".env.example",
        "copy to instance/config/env",
        "canonical env-file owner",
    )
    require_absent(
        ".env.example",
        "shared/config/aletheia.env",
        "retired shared env-file path",
    )
    require_contains(
        "shared/bin/start.sh",
        'ALETHEIA_BIN="${ALETHEIA_BIN:-aletheia}"',
        "separate binary path variable",
    )
    require_absent(
        "shared/bin/start.sh",
        'target/release/aletheia"',
        "binary path derived from ALETHEIA_ROOT",
    )
    require_contains(
        "instance.example/README.md",
        "The canonical environment file for a deployed instance is",
        "instance env-file documentation",
    )
    require_contains(
        "docs/CONFIGURATION.md",
        "`ALETHEIA_ROOT` | `taxis::Oikos` instance discovery",
        "ALETHEIA_ROOT ownership table entry",
    )


def check_onboarding_contract() -> None:
    require_contains(
        "README.md",
        "Current first run: start the server and use the TUI.",
        "current first-run path",
    )
    require_contains(
        "docs/QUICKSTART.md",
        "### Current supported path: TUI",
        "quickstart current path heading",
    )
    require_contains(
        "docs/GOLDEN-PATH.md",
        "Aletheia's v1.0 target path is desktop-first.",
        "desktop target statement",
    )
    require_contains(
        "docs/PROJECT.md",
        "Current first-run default: TUI.",
        "project interface status",
    )


def main() -> int:
    check_public_snippets()
    check_env_contract()
    check_onboarding_contract()
    if FAILURES:
        for failure in FAILURES:
            print(f"public-doc-contracts: {failure}", file=sys.stderr)
        return 1
    print("public-doc-contracts: clean")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
