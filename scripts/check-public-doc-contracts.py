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


def forbid_same_line(path: str, word_a: str, word_b: str, reason: str) -> None:
    a = word_a.lower()
    b = word_b.lower()
    for lineno, line in enumerate(read(path).splitlines(), start=1):
        low = line.lower()
        if a in low and b in low:
            FAILURES.append(f"{path}:{lineno}: forbidden {reason}: {line.strip()!r}")


def markdown_files() -> list[Path]:
    roots = [ROOT / "README.md", *sorted((ROOT / "docs").rglob("*.md"))]
    roots.extend(sorted((ROOT / "instance.example").rglob("*.md")))
    return [path for path in roots if path.is_file()]


# WHY: a maintainer-personal absolute path leaking into a public command snippet
# is not reproducible from a fresh checkout. The `(?<![\w/])` guard keeps relative
# segments such as `instance/data/` and var-prefixed `$ROOT/data/` from matching;
# only a path anchored at one of these roots is flagged.
LOCAL_PATH = re.compile(
    r"(?<![\w/])"
    r"(?:"
    r"(?P<userhome>/(?:home|Users)/(?P<user>[^\s/`\"'<>)]+))"
    r"|/data/[^\s`\"'<>)]+"
    r")"
)
# Generic substitution tokens that are portable, not a specific maintainer.
PLACEHOLDER_USERS = {
    "user", "users", "you", "youruser", "username", "name",
    "example", "someone", "$user", "${user}", "<user>", "<username>", "*",
}


# Fence languages whose contents are environment-specific config *data* the
# operator fills in (e.g. `extra_read_paths = ["/data/shared"]`), not
# copy-paste commands. Absolute example paths there are expected, not leaks.
DATA_FENCES = {
    "toml", "json", "yaml", "yml", "ini", "env", "dotenv",
    "properties", "cfg", "conf", "csv", "xml",
}


def check_public_snippets() -> None:
    for path in markdown_files():
        rel = path.relative_to(ROOT)
        in_fence = False
        fence_start = 0
        fence_lang = ""
        for lineno, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
            if line.lstrip().startswith("```"):
                if in_fence:
                    in_fence = False
                    fence_start = 0
                    fence_lang = ""
                else:
                    in_fence = True
                    fence_start = lineno
                    fence_lang = line.lstrip()[3:].strip().split()[0].lower() if line.lstrip()[3:].strip() else ""
                continue
            if not in_fence or fence_lang in DATA_FENCES:
                continue
            for match in LOCAL_PATH.finditer(line):
                user = match.group("user")
                if user is not None and user.strip("{}$<>").lower() in PLACEHOLDER_USERS:
                    continue
                FAILURES.append(
                    f"{rel}:{lineno}: maintainer-local path in public snippet "
                    f"(fence starts at line {fence_start}): {match.group(0)}"
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


# WHY: scopes scanned for env-var references that the CONFIGURATION.md contract
# table must document. Provider keys (ANTHROPIC_*, VOYAGE_*, ...) are owned by the
# credential chain and excluded by the ALETHEIA_/SEMANTIC_SCHOLAR capture below.
ENV_SCAN_GLOBS = (
    "shared/bin/*",
    "shared/bin/tests/*.bats",
    "scripts/*.sh",
    "scripts/*.py",
    "shared/templates/**/*",
    "instance.example/**/*",
)
# Matches env-var *access* idioms (python os.environ/getenv, shell $VAR/${VAR},
# systemd Environment=NAME) so plain local identifiers such as the Path variable
# `ALETHEIA_CRED = aletheia_credential_path()` are not mistaken for env vars.
ENV_ACCESS = re.compile(
    r"""(?:os\.environ(?:\.get)?\(\s*["']
        |getenv\(\s*["']
        |\$\{?
        |Environment=)
        (ALETHEIA_[A-Z0-9_]+|SEMANTIC_SCHOLAR_API_KEY)
    """,
    re.VERBOSE,
)


def reconcile_env_vars() -> None:
    contract = read("docs/CONFIGURATION.md")
    referenced: dict[str, str] = {}
    for glob in ENV_SCAN_GLOBS:
        for path in sorted(ROOT.glob(glob)):
            if not path.is_file():
                continue
            try:
                text = path.read_text(encoding="utf-8")
            except (UnicodeDecodeError, OSError):
                continue
            rel = str(path.relative_to(ROOT))
            for match in ENV_ACCESS.finditer(text):
                name = match.group(1)
                # Skip figment config-cascade keys and template substitution
                # tokens (both carry a double underscore); the cascade is
                # documented by its own ALETHEIA_<KEY>__<SUBKEY> table.
                if "__" in name:
                    continue
                referenced.setdefault(name, rel)
    for name, where in sorted(referenced.items()):
        if name not in contract:
            FAILURES.append(
                f"docs/CONFIGURATION.md: env var {name} referenced in {where} "
                f"is missing from the environment-variable contract table"
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
    # Drift guards: the current first-run surface is server + TUI; desktop is the
    # v1.0 *target*, not a present-tense canonical/default surface. These keep a
    # future edit from silently relabelling desktop as the current product.
    for doc in ("README.md", "docs/GOLDEN-PATH.md", "docs/QUICKSTART.md", "docs/PROJECT.md"):
        forbid_same_line(doc, "canonical", "desktop", "present-tense canonical-desktop claim")
    require_absent("docs/GOLDEN-PATH.md", "canonical user surface", "desktop relabelled canonical")
    require_absent("docs/GOLDEN-PATH.md", "canonical public app surface", "desktop relabelled canonical")
    require_absent("docs/PROJECT.md", "Current first-run default: desktop", "first-run default flipped to desktop")
    require_absent("docs/QUICKSTART.md", "### Current supported path: desktop", "current supported path flipped to desktop")


def check_llms_references() -> None:
    """Fail when llms.txt links to _llm files that do not exist on disk."""
    llms = (ROOT / "llms.txt").read_text(encoding="utf-8")
    # Match markdown links of the form [text](path) where path starts with _llm/
    pattern = re.compile(r"\[([^\]]*)\]\((_llm/[^\)]+)\)")
    for match in pattern.finditer(llms):
        target = match.group(2)
        if not (ROOT / target).exists():
            FAILURES.append(
                f"llms.txt: linked file does not exist: {target!r}"
            )


def main() -> int:
    check_public_snippets()
    check_env_contract()
    reconcile_env_vars()
    check_onboarding_contract()
    check_llms_references()
    if FAILURES:
        for failure in FAILURES:
            print(f"public-doc-contracts: {failure}", file=sys.stderr)
        return 1
    print("public-doc-contracts: clean")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
