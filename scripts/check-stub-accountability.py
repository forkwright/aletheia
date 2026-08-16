#!/usr/bin/env python3
"""Reject a new public stub/scaffolding site with no accountability.

aletheia#4530: several public surfaces sat reserved, stubbed, planned, or
explicitly not-wired with no record of who owns finishing them or where the
decision is tracked. The five named instances are resolved and
`crates/theatron/koilon/src/msg.rs`'s `dead_code` sites are classified in
`docs/TUI-CONTRACT.md`; this is the check that stops the next one from
landing the same way.

## Shape this looks for

A "stub-shaped site" is an `#[expect(...)]` / `#[allow(...)]` /
`#![expect(...)]` / `#![allow(...)]` attribute (bare or `cfg_attr`-wrapped;
single- or multi-line) where either:

  - its lint list contains `dead_code` and its `reason` string does not read
    as test-locality only (does not mention "test"/"tests"), or
  - its `reason` string names one of the issue's own five words/phrases:
    `stub`, `reserved`, `not yet wired`, `planned`, `future` (case-insensitive).

Test-only files (`tests/` path components, `*_test.rs`/`*_tests.rs`) are out
of scope: a helper only compiled out because it is exercised solely by test
code is a different, well-precedented pattern (100+ sites in this repo), not
the unwired-feature scaffolding #4530 is about. A `reason` that explicitly
uses one of the five keywords is flagged regardless -- the author already
called it scaffolding.

Doc-comment prose (`///`, `//!`) is deliberately NOT scanned: "future" and
"planned" appear constantly in ordinary narrative comments with no
scaffolding meaning, and a check that fires on that is disabled within a
week (the same tradeoff check-unfulfilled-expects.py names for itself). The
`reason=` string is the one place these words are written deliberately, to
explain a suppression -- that is the low-noise signal this check reads.

No Rust visibility resolution is attempted: a private dead_code allowance is
swept in alongside public ones. That only widens the net relative to the
issue's "public" framing, never narrows it -- the safe direction for a gate
whose failure mode (missing a real stub) is worse than its nuisance mode
(flagging one that is arguably already private-and-harmless).

## Accountability

A candidate site clears the gate if any of:

  1. its `reason` string contains a `#NNNN` issue reference,
  2. a `TODO(#NNNN)` / `FIXME(#NNNN)` comment sits in the few lines above it,
  3. its file is a registered CLASSIFICATION_SURFACE (a doc that classifies
     every stub site in that file the way docs/TUI-CONTRACT.md classifies
     msg.rs), or
  4. it is covered by `scripts/stub-baseline.toml`, the explicit, enumerated
     ledger of debt that predates this check.

(1)-(3) need no file edited outside the change itself. (4) is the fallback
for the sites this check was born already owing.

## Baseline is a ledger, not a wall

`scripts/stub-baseline.toml` counts occurrences per (file, item) rather than
lines, because a line number shifts on any unrelated edit above it in the
same file -- keying on line would make the gate fail on edits that never
touched a stub site. Keying on count means: paying down one of N duplicate
sites needs `count` decremented, and the baseline is checked in BOTH
directions like check-workspace-locks.py's lock/dependabot pair -- a count
that has fallen behind reality (paid-down debt) fails the same as one that
has fallen behind by growing (new debt), so the ledger cannot silently drift
stale in either direction. See check_krites_capability_matrix.py's row
completeness check for the same "drift both ways" precedent.

Usage:
    python3 scripts/check-stub-accountability.py
    python3 scripts/check-stub-accountability.py --write-baseline
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import tomllib
from collections import Counter
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
BASELINE_PATH = REPO_ROOT / "scripts" / "stub-baseline.toml"

# WHY: msg.rs's dead_code sites are governed in full by TUI-CONTRACT.md ("Every
# #[expect(dead_code, ...)] site in msg.rs falls into one of four classes") --
# membership in a file here exempts every site in it from needing its own
# baseline row or inline issue reference. Add a file here only when a doc
# makes the same file-wide completeness claim; a doc that classifies a few
# sites in a large file is not this -- use the baseline for the rest.
CLASSIFICATION_SURFACES: dict[str, str] = {
    "crates/theatron/koilon/src/msg.rs": "docs/TUI-CONTRACT.md",
}

# WHY exactly these five: the issue names them verbatim ("Inventory all
# `stub`, `reserved`, `not yet wired`, `planned`, `future`... surfaces").
# Adding words here widens what counts as scaffolding without the issue's
# authority behind it.
KEYWORDS = [
    re.compile(r"\bstub\b", re.IGNORECASE),
    re.compile(r"\breserved\b", re.IGNORECASE),
    re.compile(r"\bnot yet wired\b", re.IGNORECASE),
    re.compile(r"\bplanned\b", re.IGNORECASE),
    re.compile(r"\bfuture\b", re.IGNORECASE),
]
REASON_RE = re.compile(r'reason\s*=\s*"((?:[^"\\]|\\.)*)"')
DEAD_CODE_RE = re.compile(r"\bdead_code\b")
TEST_WORD_RE = re.compile(r"\btest", re.IGNORECASE)
ISSUE_REF_RE = re.compile(r"#\d{2,6}")
TODO_FIXME_RE = re.compile(r"\b(?:TODO|FIXME)\(#\d+\)")

TEST_PATH_RE = re.compile(r"(^|/)tests?/")
TEST_FILE_RE = re.compile(r"(?:_test|_tests|test|tests)\.rs$")

# WHY this shape, not a full Rust parser: the item right after an attribute
# is read as plain text -- an optional run of further stacked attributes or
# doc-comment lines, an optional `pub(...)`/async/unsafe/const run, an
# optional item keyword, then the identifier. It is a heuristic naming key
# for the baseline (collision-tolerant -- see COUNTER dedup below), not a
# claim about the item's true visibility or kind.
NAME_RE = re.compile(
    r"^(?:\s*(?:#!?\[[^\]]*\]|//[^\n]*))*\s*"
    r"(?:pub(?:\([^)]*\))?\s+)?"
    r"(?:async\s+|unsafe\s+|const\s+)*"
    r"(?:fn|struct|enum|trait|type|mod|static)?\s*"
    r"([A-Za-z_][A-Za-z0-9_]*)"
)


def is_test_file(rel: str) -> bool:
    if TEST_PATH_RE.search(rel):
        return True
    return bool(TEST_FILE_RE.search(rel.rsplit("/", 1)[-1]))


def tracked_rs_files() -> list[str]:
    out = subprocess.run(
        ["git", "ls-files", "-z", "crates/*.rs"],
        cwd=REPO_ROOT,
        capture_output=True,
        check=True,
    )
    return [p for p in out.stdout.decode("utf-8", "replace").split("\0") if p]


def find_attributes(text: str) -> list[tuple[int, int, str]]:
    """Return (start, end, content) for every `#[...]` / `#![...]` attribute,
    content being everything between the outer brackets.

    Bracket-depth matched (not a naive regex) so a nested `reason = "a] b"`
    or a `cfg_attr(not(test), expect(dead_code, ...))` wrapper does not
    truncate early. `"` toggles a string-skip state (with `\\`-escape
    awareness) so a bracket character INSIDE a reason string never desyncs
    the depth count -- attributes in this repo never carry char literals, so
    single-quote handling is not needed here the way check-orphaned-modules.py
    needs it for `mod` bodies.
    """
    n = len(text)
    i = 0
    out: list[tuple[int, int, str]] = []
    while i < n:
        if text[i] == "#" and i + 1 < n and (
            text[i + 1] == "[" or (text[i + 1] == "!" and i + 2 < n and text[i + 2] == "[")
        ):
            open_idx = i + 2 if text[i + 1] == "!" else i + 1
            depth = 0
            j = open_idx
            in_str = False
            while j < n:
                c = text[j]
                if in_str:
                    if c == "\\":
                        j += 2
                        continue
                    if c == '"':
                        in_str = False
                elif c == '"':
                    in_str = True
                elif c == "[":
                    depth += 1
                elif c == "]":
                    depth -= 1
                    if depth == 0:
                        j += 1
                        break
                j += 1
            out.append((i, j, text[open_idx + 1 : j - 1]))
            i = j
            continue
        i += 1
    return out


def item_name_after(text: str, end: int, lineno: int) -> str:
    window = text[end : end + 300]
    m = NAME_RE.match(window)
    return m.group(1) if m else f"<unresolved:line{lineno}>"


@dataclass
class Candidate:
    file: str
    line: int
    name: str
    reason: str | None
    inline_accounted: bool


def scan_text(rel: str, text: str) -> list[Candidate]:
    """Pure text -> candidates, with no filesystem/git access -- the part of
    the pipeline scripts/test-check-stub-accountability.py exercises directly
    against fixture strings, the same split walk_crate(repo_root=...) uses in
    check-orphaned-modules.py to stay testable without a real checkout."""
    candidates: list[Candidate] = []
    for start, end, content in find_attributes(text):
        is_dead_code = bool(DEAD_CODE_RE.search(content))
        reason_match = REASON_RE.search(content)
        reason = reason_match.group(1) if reason_match else None
        has_keyword = bool(reason and any(k.search(reason) for k in KEYWORDS))
        mentions_test = bool(reason and TEST_WORD_RE.search(reason))
        if not (has_keyword or (is_dead_code and not mentions_test)):
            continue

        lineno = text.count("\n", 0, start) + 1
        inline_accounted = bool(reason and ISSUE_REF_RE.search(reason))
        if not inline_accounted:
            # WHY a window and not just the previous line: a stub site
            # commonly carries a doc comment (`///`) between the tracking
            # note and the attribute itself.
            window_start = text.rfind("\n", 0, max(0, start - 400))
            window_start = window_start + 1 if window_start != -1 else 0
            if TODO_FIXME_RE.search(text[window_start:start]):
                inline_accounted = True

        name = item_name_after(text, end, lineno)
        candidates.append(Candidate(rel, lineno, name, reason, inline_accounted))
    return candidates


def scan_candidates() -> list[Candidate]:
    candidates: list[Candidate] = []
    for rel in tracked_rs_files():
        if is_test_file(rel):
            continue
        path = REPO_ROOT / rel
        try:
            text = path.read_text(encoding="utf-8")
        except OSError:
            continue
        candidates.extend(scan_text(rel, text))
    return candidates


def load_baseline() -> dict[tuple[str, str], int]:
    if not BASELINE_PATH.exists():
        return {}
    with BASELINE_PATH.open("rb") as fh:
        data = tomllib.load(fh)
    baseline: dict[tuple[str, str], int] = {}
    for entry in data.get("site", []):
        baseline[(entry["file"], entry["item"])] = entry["count"]
    return baseline


def write_baseline(counts: dict[tuple[str, str], int]) -> None:
    lines = [
        "# Explicit, enumerated ledger of stub-shaped sites that predate",
        "# aletheia#4530's accountability gate (scripts/check-stub-accountability.py).",
        "# Regenerate with: python3 scripts/check-stub-accountability.py --write-baseline",
        "# A new site belongs HERE only if it cannot instead carry a TODO(#NNNN)/",
        "# FIXME(#NNNN), a reason=\"...\" naming a tracking issue, or a",
        "# CLASSIFICATION_SURFACES entry -- those need no edit to this file.",
        "",
    ]
    for (file, item), count in sorted(counts.items()):
        escaped_file = file.replace("\\", "\\\\").replace('"', '\\"')
        escaped_item = item.replace("\\", "\\\\").replace('"', '\\"')
        lines.append("[[site]]")
        lines.append(f'file = "{escaped_file}"')
        lines.append(f'item = "{escaped_item}"')
        lines.append(f"count = {count}")
        lines.append("")
    BASELINE_PATH.write_text("\n".join(lines).rstrip() + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--write-baseline",
        action="store_true",
        help="regenerate scripts/stub-baseline.toml from the current tree and exit",
    )
    args = parser.parse_args()

    candidates = scan_candidates()

    unaccounted: list[Candidate] = []
    classified_count = 0
    for c in candidates:
        if c.inline_accounted:
            continue
        if CLASSIFICATION_SURFACES.get(c.file):
            classified_count += 1
            continue
        unaccounted.append(c)

    current_counts: Counter[tuple[str, str]] = Counter((c.file, c.name) for c in unaccounted)
    by_key: dict[tuple[str, str], list[Candidate]] = {}
    for c in unaccounted:
        by_key.setdefault((c.file, c.name), []).append(c)

    if args.write_baseline:
        write_baseline(dict(current_counts))
        print(f"wrote {BASELINE_PATH.relative_to(REPO_ROOT)}: {len(current_counts)} keys, "
              f"{sum(current_counts.values())} sites")
        return 0

    baseline = load_baseline()

    new_failures: list[str] = []
    for key, count in sorted(current_counts.items()):
        allowed = baseline.get(key, 0)
        if count > allowed:
            file, name = key
            new_sites = by_key[key][allowed:]
            for site in new_sites:
                reason_note = f' reason="{site.reason}"' if site.reason else " (no reason string)"
                new_failures.append(f"{site.file}:{site.line}: `{site.name}`{reason_note}")

    stale_failures: list[str] = []
    for key, allowed in sorted(baseline.items()):
        actual = current_counts.get(key, 0)
        if actual < allowed:
            file, name = key
            stale_failures.append(
                f"{file}: `{name}` baseline count is {allowed}, tree has {actual}"
            )

    # WHY print only the earned class: a caller who sees the wrong remedy
    # loses more time than one who sees no remedy at all.
    if new_failures:
        print("stub-accountability: new stub-shaped site(s) with no accountability:", file=sys.stderr)
        for f in new_failures:
            print(f"  {f}", file=sys.stderr)
        print(
            "\nGive each one a `TODO(#NNNN)`/`FIXME(#NNNN)` above it, a "
            'reason="..." naming the tracking issue (`#NNNN`), or (for a file '
            "governed end-to-end by a classification doc like docs/TUI-CONTRACT.md) "
            "register that file in CLASSIFICATION_SURFACES. Only pre-existing debt "
            "belongs in scripts/stub-baseline.toml -- regenerate it with "
            "--write-baseline and review the diff before adding a new site there.",
            file=sys.stderr,
        )
        return 1

    if stale_failures:
        print("stub-accountability: baseline is stale (paid-down debt not reflected):", file=sys.stderr)
        for f in stale_failures:
            print(f"  {f}", file=sys.stderr)
        print(
            "\nscripts/stub-baseline.toml claims more sites than the tree has for "
            "these keys. Regenerate with --write-baseline and commit the smaller "
            "ledger -- a baseline that overclaims can hide a real regression "
            "elsewhere in the same key.",
            file=sys.stderr,
        )
        return 1

    print(
        f"stub-accountability: clean -- {len(candidates)} stub-shaped sites scanned, "
        f"{classified_count} covered by CLASSIFICATION_SURFACES, "
        f"{sum(current_counts.values())} covered by scripts/stub-baseline.toml "
        f"({len(current_counts)} keys), 0 unaccounted"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
