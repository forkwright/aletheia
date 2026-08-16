#!/usr/bin/env python3
"""Check that docs/scripts citing hermeneus LLM metric names match what is
actually registered and exposed.

`docs/RUNBOOK.md`, `docs/OBSERVABILITY-AUDIT.md`, and `scripts/health-monitor.sh`
each hand-copy Prometheus metric names from `crates/hermeneus/src/metrics.rs`
rather than deriving them, and drifted independently (aletheia#4526):
`aletheia_llm_cost_total` vs the registered `aletheia_llm_cost_usd_total`, and
a split `aletheia_llm_input_tokens_total`/`aletheia_llm_output_tokens_total`
pair that was replaced by a single `aletheia_llm_tokens_total{direction=...}`
family. The ground truth for what is actually exposed is
`register_exposes_all_metric_families`'s fragment list in metrics.rs -- that
test asserts each fragment against real encoder output, so a drift there
already fails `cargo test`; this script makes the same list check the prose
surfaces that test cannot reach.

Usage:
    python3 scripts/check-metrics-doc.py --check

--check exits 0 when every `aletheia_llm_*` token in the watched surfaces
resolves to a real exposed metric family, and non-zero naming the file/line
and the unresolvable token otherwise.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
METRICS_RS = REPO_ROOT / "crates" / "hermeneus" / "src" / "metrics.rs"

# WHY: the doc/script surfaces known to hand-copy hermeneus metric names.
# Extend this list if another surface starts citing `aletheia_llm_*` names.
WATCHED_SURFACES = [
    REPO_ROOT / "docs" / "RUNBOOK.md",
    REPO_ROOT / "docs" / "OBSERVABILITY-AUDIT.md",
    REPO_ROOT / "docs" / "OBSERVABILITY.md",
    REPO_ROOT / "scripts" / "health-monitor.sh",
]

# Histogram families expose `_bucket`/`_sum`/`_count` suffixes in addition to
# the base name asserted in metrics.rs's fragment list.
HISTOGRAM_SUFFIXES = ("_bucket", "_sum", "_count")

TOKEN_RE = re.compile(r"aletheia_llm_[A-Za-z0-9_]*")
FRAGMENT_FN_RE = re.compile(
    r"fn register_exposes_all_metric_families\b.*?for fragment in \[(.*?)\]",
    re.DOTALL,
)
STRING_LITERAL_RE = re.compile(r'"([^"]+)"')


def ground_truth_names() -> set[str]:
    """Extract the encoder-verified metric-name fragments from metrics.rs's
    own test, so this script derives from the same source that already fails
    `cargo test` on drift rather than re-declaring the list by hand."""
    text = METRICS_RS.read_text(encoding="utf-8")
    match = FRAGMENT_FN_RE.search(text)
    if not match:
        raise RuntimeError(
            f"could not find register_exposes_all_metric_families's fragment "
            f"list in {METRICS_RS} -- has the test been renamed or restructured?"
        )
    return set(STRING_LITERAL_RE.findall(match.group(1)))


def is_known(token: str, ground_truth: set[str]) -> bool:
    if token in ground_truth:
        return True
    return any(
        token.startswith(name) and token[len(name) :] in HISTOGRAM_SUFFIXES
        for name in ground_truth
    )


def find_drift(ground_truth: set[str]) -> list[str]:
    problems: list[str] = []
    for path in WATCHED_SURFACES:
        if not path.exists():
            problems.append(f"{path}: watched surface no longer exists")
            continue
        for lineno, line in enumerate(
            path.read_text(encoding="utf-8").splitlines(), start=1
        ):
            for token in TOKEN_RE.findall(line):
                if not is_known(token, ground_truth):
                    problems.append(
                        f"{path.relative_to(REPO_ROOT)}:{lineno}: "
                        f"`{token}` is not a real exposed metric family "
                        f"(known: {', '.join(sorted(ground_truth))})"
                    )
    return problems


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check",
        action="store_true",
        help="check watched surfaces against metrics.rs and exit non-zero on drift",
    )
    args = parser.parse_args()

    if not args.check:
        parser.print_help()
        return 1

    ground_truth = ground_truth_names()
    problems = find_drift(ground_truth)
    if problems:
        print("Stale Prometheus metric names found:", file=sys.stderr)
        for problem in problems:
            print(f"  {problem}", file=sys.stderr)
        print(
            f"\nCanonical source: {METRICS_RS.relative_to(REPO_ROOT)}'s "
            "register_exposes_all_metric_families test.",
            file=sys.stderr,
        )
        return 1

    print(f"OK: {len(WATCHED_SURFACES)} surfaces match registered metric names.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
