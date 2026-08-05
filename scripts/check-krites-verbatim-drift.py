#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Calibrated verbatim-drift metric for crates/krites against pinned upstream.

WHY: the prior metric (order-insensitive non-blank line multiset,
common / upstream_nonblank) is refuted by measurement — it scores 45.4% on
storage/fjall_backend.rs (no upstream counterpart at all), 42.0% on
fixed_rule/algos/kcore.rs (an algorithm upstream lacks), and query/graph.rs
is 24% punctuation/use/attribute lines before any logic matches. Two
independent Rust implementations of the same problem domain clear 20% on
braces and imports alone.

This metric: token-shingle Jaccard over >=8-gram normalized token streams,
built only from lines that are (a) not punctuation-only, (b) not part of a
`use` statement, (c) not an attribute, and (d) carry at least
MIN_IDENTIFIER_TOKENS_PER_LINE identifier-shaped tokens. Computed against
the pinned vendored snapshot at upstream-snapshot/cozo-core-src (see its
NOTICE.md) — never a network fetch.

Report-only. Do not add `--strict` to any CI invocation until every one of
the following holds (PLAN.md wave 0.3 / kill criterion #1):

  1. crates/krites/PROVENANCE.toml (wave 0.1) exists, has one row per
     src/ file, and CI already fails on ledger creep independently of this
     script — a --strict drift failure must never be the FIRST signal that
     the ledger is stale.
  2. This script has been run in --calibrate mode against the CURRENT
     upstream snapshot (re-run after any snapshot update) and
     CALIBRATED_THRESHOLD sits above the freshly measured known-original
     max, with the margin re-stated in the constant's comment.
  3. The OVERLAP CHECK in --calibrate output is read and accepted, not
     just present — see the NOTE it prints. A low score is not proof of
     originality (see docstring on `run_calibration`'s overlap section);
     --strict only ever fires on HIGH scores (kill-criterion direction),
     never used to auto-clear a file as sovereign.
  4. The full-report mode (default, no flags) has been run at least once
     across the entire crate and every file scoring above threshold has
     been individually reviewed — not bulk-waived — because at promotion
     time the threshold starts gating NEW files, and a pre-existing
     over-threshold file would otherwise block unrelated PRs.
  5. Wave 0.3's kill-criterion framing is honored: tightest on the
     datalog.pest replacement, but not zero-tolerance — a grammar file's
     rule-definition lines are largely dictated by the language being
     described, so --strict promotion should carry a documented per-file-
     type allowance for grammar files rather than one global cutoff.

Run standalone:
    uv run scripts/check-krites-verbatim-drift.py                 # full report
    uv run scripts/check-krites-verbatim-drift.py --calibrate      # calibration run
    uv run scripts/check-krites-verbatim-drift.py --file <path>    # single file

Exits 0 always, except with --strict (see PROMOTION CRITERIA — not yet met).
"""

from __future__ import annotations

import argparse
import logging
import re
import sys
from dataclasses import dataclass
from pathlib import Path

LOGGER = logging.getLogger("check-krites-verbatim-drift")

REPO_ROOT = Path(__file__).resolve().parents[1]
KRITES_SRC = REPO_ROOT / "crates" / "krites" / "src"
UPSTREAM_SRC = REPO_ROOT / "crates" / "krites" / "upstream-snapshot" / "cozo-core-src"

SHINGLE_SIZE = 8
MIN_IDENTIFIER_TOKENS_PER_LINE = 3

# WHY: derived by running --calibrate against the pinned snapshot (see
# crates/krites/upstream-snapshot/NOTICE.md for the pinned upstream commit).
# The known-original set's highest score (global max across the whole
# upstream corpus, i.e. worst case) measured 0.0529
# (fixed_rule/algos/kcore.rs vs upstream fixed_rule/algos/pagerank.rs — two
# graph algorithms sharing loop/accumulator boilerplate, nowhere near a real
# match). 0.10 clears that with a 0.047 margin (~1.9x). A changed upstream
# snapshot invalidates this constant until recalibrated — run --calibrate
# again and update both the constant and this comment together.
CALIBRATED_THRESHOLD = 0.10

# Known-original: files with no genuine upstream lineage (aletheia-authored
# engine surface sitting inside the derived crate). The calibration set
# named in PLAN.md wave 0.3.
KNOWN_ORIGINAL_FILES = [
    "storage/fjall_backend.rs",
    "fixed_rule/algos/kcore.rs",
    "fixed_rule/csr/mod.rs",
    "fixed_rule/csr/page_rank.rs",
    "hot_reload.rs",
    "async_surface.rs",
]

# ---------------------------------------------------------------------------
# Line classification
# ---------------------------------------------------------------------------

_ATTRIBUTE_RE = re.compile(r"^#!?\[.*\]$")
_PUNCTUATION_ONLY_RE = re.compile(r"^[^A-Za-z0-9_]*$")
_IDENTIFIER_RE = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")

_TOKEN_RE = re.compile(
    r"""
    r\#*"(?:[^"]|"(?!\#*"))*"\#*          # raw string literal (best-effort)
  | "(?:\\.|[^"\\])*"                      # string literal
  | '(?:\\.|[^'\\])'                       # char literal
  | [A-Za-z_][A-Za-z0-9_]*                 # identifier / keyword
  | \d+(?:\.\d+)?(?:[eE][+-]?\d+)?         # number
  | ::|->|=>|==|!=|<=|>=|&&|\|\||\+=|-=|\*=|/=|%=  # multi-char operators
  | .                                       # single-char punctuation (dotall off)
    """,
    re.VERBOSE,
)


class _UseTracker:
    """WHY: a `use` statement can span multiple lines (`use foo::{\\n  bar,\\n};`).
    Tracks brace balance from the opening `use` line through to the closing `;`
    so every line of the statement is excluded, not just the first."""

    def __init__(self) -> None:
        self._active = False
        self._depth = 0

    def consume(self, stripped: str) -> bool:
        if not self._active:
            if not stripped.startswith("use "):
                return False
            self._active = True
            self._depth = 0
        self._depth += stripped.count("{") - stripped.count("}")
        if self._depth <= 0 and stripped.rstrip().endswith(";"):
            self._active = False
        return True


def eligible_lines(text: str) -> list[str]:
    """Return the subset of lines that count toward the shingle stream."""
    out: list[str] = []
    use_tracker = _UseTracker()
    for raw in text.splitlines():
        stripped = raw.strip()
        if not stripped:
            continue
        if use_tracker.consume(stripped):
            continue
        if _ATTRIBUTE_RE.match(stripped):
            continue
        if _PUNCTUATION_ONLY_RE.match(stripped):
            continue
        if len(_IDENTIFIER_RE.findall(stripped)) < MIN_IDENTIFIER_TOKENS_PER_LINE:
            continue
        out.append(stripped)
    return out


def tokenize(text: str) -> list[str]:
    lines = eligible_lines(text)
    tokens: list[str] = []
    for line in lines:
        tokens.extend(m.group(0) for m in _TOKEN_RE.finditer(line) if not m.group(0).isspace())
    return tokens


def shingles(tokens: list[str]) -> frozenset[tuple[str, ...]]:
    if len(tokens) < SHINGLE_SIZE:
        return frozenset()
    return frozenset(
        tuple(tokens[i : i + SHINGLE_SIZE]) for i in range(len(tokens) - SHINGLE_SIZE + 1)
    )


def jaccard(a: frozenset, b: frozenset) -> float:
    if not a and not b:
        return 0.0
    inter = len(a & b)
    union = len(a | b)
    return inter / union if union else 0.0


# ---------------------------------------------------------------------------
# Corpus loading
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class FileShingles:
    relpath: str
    path: Path
    token_count: int
    shingle_set: frozenset


def _load_corpus(root: Path) -> dict[str, FileShingles]:
    corpus: dict[str, FileShingles] = {}
    for path in sorted(root.rglob("*")):
        if not path.is_file():
            continue
        if path.suffix not in (".rs", ".pest"):
            continue
        text = path.read_text(encoding="utf-8", errors="replace")
        tokens = tokenize(text)
        relpath = str(path.relative_to(root))
        corpus[relpath] = FileShingles(relpath, path, len(tokens), shingles(tokens))
    return corpus


def load_krites_corpus() -> dict[str, FileShingles]:
    return _load_corpus(KRITES_SRC)


def load_upstream_corpus() -> dict[str, FileShingles]:
    if not UPSTREAM_SRC.is_dir():
        LOGGER.error("upstream snapshot missing at %s", UPSTREAM_SRC)
        raise SystemExit(2)
    return _load_corpus(UPSTREAM_SRC)


# ---------------------------------------------------------------------------
# Scoring
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class Score:
    relpath: str
    best_match: str | None
    jaccard: float
    shared_shingles: int
    file_shingles: int
    match_shingles: int
    paired: bool  # True if best_match is the same-relative-path upstream file


def global_best_match(fs: FileShingles, upstream: dict[str, FileShingles]) -> Score:
    best_path: str | None = None
    best_score = 0.0
    best_shared = 0
    best_upstream_count = 0
    for up_relpath, up in upstream.items():
        shared = len(fs.shingle_set & up.shingle_set)
        if shared == 0 and up.shingle_set:
            continue
        score = jaccard(fs.shingle_set, up.shingle_set)
        if score > best_score:
            best_score = score
            best_path = up_relpath
            best_shared = shared
            best_upstream_count = len(up.shingle_set)
    return Score(
        relpath=fs.relpath,
        best_match=best_path,
        jaccard=best_score,
        shared_shingles=best_shared,
        file_shingles=len(fs.shingle_set),
        match_shingles=best_upstream_count,
        paired=(best_path == fs.relpath),
    )


def paired_score(fs: FileShingles, upstream: dict[str, FileShingles]) -> Score | None:
    up = upstream.get(fs.relpath)
    if up is None:
        return None
    shared = len(fs.shingle_set & up.shingle_set)
    return Score(
        relpath=fs.relpath,
        best_match=fs.relpath,
        jaccard=jaccard(fs.shingle_set, up.shingle_set),
        shared_shingles=shared,
        file_shingles=len(fs.shingle_set),
        match_shingles=len(up.shingle_set),
        paired=True,
    )


# ---------------------------------------------------------------------------
# Modes
# ---------------------------------------------------------------------------


def run_calibration() -> int:
    krites = load_krites_corpus()
    upstream = load_upstream_corpus()

    print("=== Calibration: known-original set vs FULL upstream corpus (global max) ===")
    print(f"{'file':<45} {'best-match upstream':<40} {'shared':>7} {'file_sh':>8} {'match_sh':>9} {'jaccard':>8}")
    original_scores: list[float] = []
    for relpath in KNOWN_ORIGINAL_FILES:
        fs = krites.get(relpath)
        if fs is None:
            LOGGER.error("known-original file missing from krites/src: %s", relpath)
            return 2
        score = global_best_match(fs, upstream)
        original_scores.append(score.jaccard)
        best_label = score.best_match if score.best_match is not None else "<no-shared-shingles>"
        print(
            f"{relpath:<45} {best_label:<40} {score.shared_shingles:>7} "
            f"{score.file_shingles:>8} {score.match_shingles:>9} {score.jaccard:>8.4f}"
        )

    highest_original = max(original_scores) if original_scores else 0.0
    print()
    print(f"Highest known-original score (global max, worst case): {highest_original:.4f}")

    print()
    print("=== Known-derived population: paired same-relative-path scores ===")
    derived: list[Score] = []
    for relpath, fs in krites.items():
        s = paired_score(fs, upstream)
        if s is None:
            continue
        derived.append(s)
    if derived:
        by_score = sorted(derived, key=lambda s: s.jaccard)
        vals = [s.jaccard for s in by_score]
        print(f"paired files (same relative path exists upstream): {len(derived)}")
        print(f"min={vals[0]:.4f}  median={vals[len(vals)//2]:.4f}  "
              f"max={vals[-1]:.4f}  mean={sum(vals)/len(vals):.4f}")

        overlap = [s for s in by_score if s.jaccard <= highest_original]
        print()
        print(
            f"OVERLAP CHECK — known-derived files scoring <= known-original max "
            f"({highest_original:.4f}): {len(overlap)} / {len(derived)}"
        )
        for s in overlap:
            print(f"  {s.relpath:<45} jaccard={s.jaccard:.4f}  file_sh={s.file_shingles}  match_sh={s.match_shingles}")
        if overlap:
            print(
                "  NOTE: a low score on a file with genuine upstream lineage is not proof of\n"
                "  originality — it means that file has already been rewritten far enough that\n"
                "  little literal token sequence survives. This metric detects verbatim SURVIVAL,\n"
                "  not authorship; a low score cannot promote a ledger row derived -> sovereign on\n"
                "  its own. It also means the metric's real discriminating power is directional\n"
                "  (originals never score high) rather than a bidirectional derived/original\n"
                "  classifier at the low end. See PROMOTION CRITERIA."
            )
    else:
        print("no paired same-relative-path files found — cannot compute a known-derived population")

    print()
    print(f"CALIBRATED_THRESHOLD in this script: {CALIBRATED_THRESHOLD:.4f}")
    if CALIBRATED_THRESHOLD <= highest_original:
        LOGGER.error(
            "CALIBRATED_THRESHOLD (%.4f) does not clear the known-original max (%.4f)",
            CALIBRATED_THRESHOLD, highest_original,
        )
        return 1

    print(f"margin above known-original max: {CALIBRATED_THRESHOLD - highest_original:.4f}")
    return 0


def run_report(strict: bool) -> int:
    krites = load_krites_corpus()
    upstream = load_upstream_corpus()

    rows: list[Score] = []
    for relpath, fs in sorted(krites.items()):
        paired = paired_score(fs, upstream)
        rows.append(paired if paired is not None else global_best_match(fs, upstream))

    rows.sort(key=lambda r: r.jaccard, reverse=True)

    print(f"{'file':<50} {'match':<45} {'paired':>6} {'jaccard':>8}")
    over_threshold = []
    for r in rows:
        match_label = r.best_match if r.best_match is not None else "<no-shared-shingles>"
        print(f"{r.relpath:<50} {match_label:<45} {r.paired!s:>6} {r.jaccard:>8.4f}")
        if r.jaccard > CALIBRATED_THRESHOLD:
            over_threshold.append(r)

    print()
    print(f"files scored: {len(rows)}")
    print(f"files above calibrated threshold ({CALIBRATED_THRESHOLD:.4f}): {len(over_threshold)}")
    print("REPORT-ONLY: this run never fails the build unless --strict is passed. See")
    print("PROMOTION CRITERIA in this script's module docstring / PLAN.md wave 0.3 before")
    print("ever passing --strict in CI.")

    if strict and over_threshold:
        return 1
    return 0


def run_single(relpath: str) -> int:
    krites = load_krites_corpus()
    upstream = load_upstream_corpus()
    fs = krites.get(relpath)
    if fs is None:
        LOGGER.error("file not found under crates/krites/src: %s", relpath)
        return 2
    paired = paired_score(fs, upstream)
    score = paired if paired is not None else global_best_match(fs, upstream)
    print(f"file: {score.relpath}")
    print(f"best match: {score.best_match} (paired={score.paired})")
    print(f"shared shingles: {score.shared_shingles}")
    print(f"file shingles: {score.file_shingles}  match shingles: {score.match_shingles}")
    print(f"jaccard: {score.jaccard:.4f}")
    print(f"calibrated threshold: {CALIBRATED_THRESHOLD:.4f}")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--calibrate", action="store_true", help="run the calibration report")
    parser.add_argument("--file", type=str, default=None, help="score a single krites/src-relative file")
    parser.add_argument(
        "--strict",
        action="store_true",
        help="exit 1 if any file exceeds the calibrated threshold (NOT for CI use — see PROMOTION CRITERIA)",
    )
    args = parser.parse_args()

    if args.calibrate:
        return run_calibration()
    if args.file:
        return run_single(args.file)
    return run_report(strict=args.strict)


if __name__ == "__main__":
    logging.basicConfig(format="%(message)s", level=logging.INFO, stream=sys.stderr)
    raise SystemExit(main())
