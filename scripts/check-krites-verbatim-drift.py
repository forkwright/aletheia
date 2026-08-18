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

`--strict` GATES, on one narrow condition: a row that is `sovereign` AND
records `replaced_upstream_path = "none"` AND scores above CALIBRATED_THRESHOLD.
Nothing else fails, ever.

That predicate is what made promotion possible. The original criteria (wave 0.3)
required every over-threshold file to be individually reviewed first, because a
global cutoff "starts gating NEW files, and a pre-existing over-threshold file
would otherwise block unrelated PRs". Under a global cutoff that was 62 files.
Cross-referenced against the ledger it is none, because:

  - A `derived` or `dual` row scoring high is the metric WORKING. The row
    declares upstream lineage; a high figure is the expected reading.
  - A `sovereign` row recording what it replaced already publishes a measured
    figure that check-krites-provenance.py recomputes independently. The number
    is evidence (storage/temp.rs states 33.2%), not a defect.
  - A `sovereign` row recording no predecessor asserts there is nothing to
    compare against. A high score refutes that assertion, which is the one thing
    this metric can prove. That is the SOVEREIGN_VERIFY_MAP-by-omission hole.

The remaining criteria are still live and still binding:

  1. PROVENANCE.toml gates independently -- a drift failure must never be the
     FIRST signal the ledger is stale. `_load_ledger_rows` fails closed on a
     missing or unparsable ledger for this reason.
  2. Re-run --calibrate after ANY snapshot update or filter change, and restate
     the margin in CALIBRATED_THRESHOLD's comment. Last measured: known-original
     max 0.0881 against a 0.1700 threshold, margin 0.0819.
  3. The OVERLAP CHECK is read, not just present: a LOW score never promotes a
     row derived -> sovereign. This gate only ever fires on HIGH scores.

Criterion 5's per-file-type allowance for grammars is superseded rather than
implemented: it existed so a file whose content is dictated by an external
referent would not fail on a high score. Under this predicate such a file does
not need an exemption -- it needs its predecessor recorded, after which its
figure is published rather than waived. stopwords.rs was the live example: MIT
stopwords-iso data, word-for-word unchanged from what it replaced, sitting in
the ledger at 0.0/none while measuring 0.94. It now records 76.6% against its
predecessor, which is the honest statement and needs no allowance.

Run standalone:
    uv run scripts/check-krites-verbatim-drift.py                 # full report
    uv run scripts/check-krites-verbatim-drift.py --calibrate      # calibration run
    uv run scripts/check-krites-verbatim-drift.py --file <path>    # single file

Exits 0 except with --strict, which fails only on the condition above.
"""

from __future__ import annotations

import argparse
import logging
import re
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

# NOTE(#5956): the one import this otherwise-standalone script takes from the ledger
# library, and it is deliberate. What counts as "the generated notice block" must have a
# single definition, or this metric and verbatim_pct would exclude different things and the
# divergence would show up as an unexplained figure rather than as an error. The library
# imports only the standard library, so `dependencies = []` above still holds.
from krites_provenance_lib import strip_generated_notice  # noqa: E402

LOGGER = logging.getLogger("check-krites-verbatim-drift")

REPO_ROOT = Path(__file__).resolve().parents[1]
KRITES_SRC = REPO_ROOT / "crates" / "krites" / "src"
UPSTREAM_SRC = REPO_ROOT / "crates" / "krites" / "upstream-snapshot" / "cozo-core-src"

SHINGLE_SIZE = 8
MIN_IDENTIFIER_TOKENS_PER_LINE = 3

# WHY: derived by running --calibrate against the pinned snapshot (see
# crates/krites/upstream-snapshot/NOTICE.md for the pinned upstream commit,
# 481af058ab, re-vendored from the earlier v0.7.6-tag snapshot). The
# known-original set's highest score (global max across the whole upstream
# corpus, i.e. worst case) measured 0.0881 (storage/fjall_backend.rs vs
# upstream's newly-added storage/newrocks.rs — both are RocksDB-storage-
# backend implementations sharing generic put/get/delete/batch boilerplate,
# nowhere near a real match; newrocks.rs did not exist in the v0.7.6
# snapshot, which is why this max moved up from a prior 0.0529). 0.17
# clears that with a 0.082 margin (~1.9x, matching the safety factor of the
# original calibration). A changed upstream snapshot invalidates this
# constant until recalibrated — run --calibrate again and update both the
# constant and this comment together.
CALIBRATED_THRESHOLD = 0.17

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


# WHY a regex rather than startswith("use "): a re-export is written
# `pub use ..` or `pub(crate) use ..`, and the plain prefix check missed every
# one of them. That contradicted this module's own stated filter -- imports are
# excluded because "two independent Rust implementations of the same problem
# domain clear 20% on braces and imports alone", and a re-export is an import
# by another name. Measured, it mattered: fixed_rule/algos/mod.rs is 40 eligible
# lines of which 18 are `mod x;` and most of the rest were `pub(crate) use
# X::Y;`, scoring 0.6505 against upstream on a file that declares a module list
# and re-exports it.
# WHY module declarations are excluded on the same grounds: `pub(crate) mod bfs;`
# names a file that exists. There is no authored token sequence in it that could
# survive from upstream -- two implementations of the same algorithm set have
# identical module lists whoever wrote them. Measured, fixed_rule/algos/mod.rs is
# 18 such lines out of 22 eligible after the re-export fix, and scored 0.6505 on
# them; a manifest cannot be rewritten to be more original without renaming the
# things it lists.
_MOD_DECL_RE = re.compile(r"^(?:pub(?:\([^)]*\))?\s+)?mod\s+[A-Za-z_][A-Za-z0-9_]*\s*;$")

_USE_START_RE = re.compile(r"^(?:pub(?:\([^)]*\))?\s+)?use\s")


class _UseTracker:
    """WHY: a `use` statement can span multiple lines (`use foo::{\\n  bar,\\n};`).
    Tracks brace balance from the opening `use` line through to the closing `;`
    so every line of the statement is excluded, not just the first."""

    def __init__(self) -> None:
        self._active = False
        self._depth = 0

    def consume(self, stripped: str) -> bool:
        if not self._active:
            if not _USE_START_RE.match(stripped):
                return False
            self._active = True
            self._depth = 0
        self._depth += stripped.count("{") - stripped.count("}")
        if self._depth <= 0 and stripped.rstrip().endswith(";"):
            self._active = False
        return True


def eligible_lines(text: str) -> list[str]:
    """Return the subset of lines that count toward the shingle stream.

    WARNING(#5956): the generated MPL Exhibit A block is removed before any line is
    classified. This filter keeps ordinary comment lines -- only punctuation-only, `use`,
    `mod` and attribute lines are dropped -- and 122 of the upstream files carry the same
    notice in their own header, so a per-file notice added to a derived file would
    shingle-match upstream's copy and raise this metric on licence boilerplate. That is the
    corruption strip_generated_notice exists to prevent in verbatim_pct, reached through a
    second instrument, so it is excluded from the same single definition rather than a
    second copy of one.
    """
    out: list[str] = []
    use_tracker = _UseTracker()
    for raw in strip_generated_notice(text).splitlines():
        stripped = raw.strip()
        if not stripped:
            continue
        if use_tracker.consume(stripped):
            continue
        if _MOD_DECL_RE.match(stripped):
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
        # WHY: shared == 0 implies jaccard == 0 unconditionally — whether
        # up.shingle_set is empty (jaccard's not-a-and-not-b special case,
        # or a zero-numerator division) or non-empty (zero-numerator
        # division). A zero score can never exceed best_score (init 0.0,
        # strictly increasing thereafter), so skipping the union-formation
        # in jaccard() for every zero-overlap candidate is always safe —
        # previously this only skipped when up.shingle_set was non-empty,
        # leaving empty-upstream-file candidates to fall through and
        # recompute the same 0.0 the guard exists to shortcut.
        if shared == 0:
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
            "OVERLAP CHECK — known-derived files scoring <= known-original max "
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


def _load_ledger_rows() -> dict[str, dict]:
    """Read PROVENANCE.toml once, for the strict gate's status lookup.

    Fails closed: strict mode compares against ledger rows, so an unreadable or
    absent ledger must stop the run rather than silently find nothing to gate.
    Criterion 1 of the promotion contract is that the ledger already gates
    independently -- a drift failure must never be the FIRST signal it is stale,
    and an empty lookup here would invert that.
    """
    path = KRITES_SRC.parent / "PROVENANCE.toml"
    if not path.exists():
        raise SystemExit(f"{path} is absent; --strict compares against it and cannot proceed.")
    try:
        data = tomllib.loads(path.read_text())
    except tomllib.TOMLDecodeError as exc:
        raise SystemExit(f"{path} could not be parsed, so --strict has nothing to gate: {exc}") from exc
    return {r["path"]: r for r in data.get("file", []) if isinstance(r, dict) and "path" in r}


def _asserts_no_predecessor(relpath: str) -> bool:
    """Whether the ledger row for `relpath` is sovereign AND records no predecessor.

    WHY strict fires only on this shape, rather than on any high-scoring row:

    - A `derived` or `dual` row scoring high is the metric WORKING. That row
      declares upstream lineage; a high figure is the expected reading and must
      never fail a build. 62 files are in this position today, which is what made
      a global cutoff look like 62 files to review rather than a usable gate.
    - A `sovereign` row that records `replaced_upstream_path` already publishes a
      measured figure in PROVENANCE.toml and NOTICE.md, and
      check-krites-provenance.py recomputes it independently. The number is
      evidence, not a defect -- storage/temp.rs states 33.2% and is honest.
    - A `sovereign` row recording `replaced_upstream_path = "none"` is asserting
      there is nothing to compare it against. A high drift score REFUTES that
      assertion directly, which is the one thing this metric can prove.

    That is precisely the SOVEREIGN_VERIFY_MAP-by-omission hole: a path absent
    from the map is assigned 0.0, and nothing checked that a file claiming
    sovereignty had an entry. This closes it without an exemption list, because
    the fix for a real hit is to record the predecessor rather than to waive it.
    """
    row = _load_ledger_rows().get(relpath)
    if row is None:
        return False
    return row.get("status") == "sovereign" and row.get("replaced_upstream_path", "none") == "none"


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

    if not strict:
        return 0

    unmeasured = [r for r in over_threshold if _asserts_no_predecessor(r.relpath)]
    if not unmeasured:
        print()
        print(
            f"STRICT: no `sovereign` row asserting no predecessor scores above "
            f"{CALIBRATED_THRESHOLD:.4f}. ({len(over_threshold)} file(s) are above it and all "
            "either declare upstream lineage or already record a measurement against what they "
            "replaced.)"
        )
        return 0

    print()
    print("STRICT FAILURE: these rows claim to be sovereign with nothing to measure against,")
    print("while scoring above the calibrated threshold against a real upstream file:")
    for r in unmeasured:
        print(f"  {r.jaccard:.4f}  {r.relpath}  (vs upstream {r.best_match})")
    print()
    print("Record what the file replaced in SOVEREIGN_VERIFY_MAP and regenerate, so the row")
    print("carries a measured figure instead of an asserted 0.0 -- or, if it genuinely has no")
    print("predecessor, this score is evidence that it does.")
    return 1


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
