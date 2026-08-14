#!/usr/bin/env python3
"""Report-only: flag actions/checkout steps carrying the fetch-depth:0 +
short-timeout signature seen in #6756 (check-trailer, detect CodeQL scope,
attestation-verify each timed out at ~5m0Xs cloning full history for a job
that never needed it).

WHY report-only, not gating: fetch-depth:0 with no filter is not itself
wrong -- gate-attestation.yml's gate-coverage-scripts carries it
deliberately (#6757 triage: the job reads nearly every tracked file at HEAD
via check-conflict-markers.py, so blob:none buys little and a hard gate here
would nag a documented, reasoned exception). This script surfaces the
pattern for human triage the way #6756's three incidents were each found
only by reading a job log after something else looked broken; it does not
decide whether filter: blob:none is the right fix for a given job.

WHY the timeout threshold is a constant here, not derived: no other file in
this repo declares "what counts as a short CI timeout" as data, and this
script is the first consumer of that concept.
"""

from __future__ import annotations

import glob
import sys
from pathlib import Path

import yaml

REPO_ROOT = Path(__file__).resolve().parent.parent
WORKFLOWS_DIR = REPO_ROOT / ".github" / "workflows"

# WHY 10: #6756's three incidents ran at timeout-minutes: 5. Padding to 10
# catches jobs one budget-doubling away from the same failure mode without
# flagging the 15-90 minute jobs that need fetch-depth:0 for a real reason
# (secret-scan, gitleaks, online-tests, release test) and are timed for
# compute, not clone.
SHORT_TIMEOUT_MINUTES = 10


def iter_checkout_steps(workflow: dict) -> list[tuple[str, dict, dict]]:
    """Yield (job_name, job, step) for every actions/checkout step."""
    found = []
    for job_name, job in (workflow.get("jobs") or {}).items():
        if not isinstance(job, dict):
            continue
        for step in job.get("steps") or []:
            uses = step.get("uses", "")
            if isinstance(uses, str) and uses.startswith("actions/checkout"):
                found.append((job_name, job, step))
    return found


def find_risks(path: Path) -> list[str]:
    workflow = yaml.safe_load(path.read_text())
    if not isinstance(workflow, dict):
        return []

    risks = []
    for job_name, job, step in iter_checkout_steps(workflow):
        with_block = step.get("with") or {}
        fetch_depth = with_block.get("fetch-depth")
        has_filter = "filter" in with_block
        if fetch_depth not in (0, "0") or has_filter:
            continue

        timeout = job.get("timeout-minutes")
        if timeout is None or timeout > SHORT_TIMEOUT_MINUTES:
            continue

        risks.append(
            f"{path.name}: job '{job_name}' checks out fetch-depth:0 with "
            f"timeout-minutes:{timeout} and no filter -- an overrun reports "
            f"`cancelled`, indistinguishable from a superseded run (#6756)."
        )
    return risks


def main() -> int:
    all_risks = []
    for filename in sorted(glob.glob(str(WORKFLOWS_DIR / "*.yml"))):
        all_risks.extend(find_risks(Path(filename)))

    if all_risks:
        print("### checkout filter risk (fetch-depth:0 + short timeout, no filter)")
        print()
        for risk in all_risks:
            print(f"- {risk}")
        print()
        print(
            "Report-only (#6757): each flagged job needs the same read -- what "
            "does it actually read -- that #6756's fix and #6757's triage applied "
            "by hand. Not every flag wants filter: blob:none; a job reading "
            "nearly the whole tree at HEAD (e.g. gate-coverage-scripts) may not "
            "benefit even at fetch-depth:0."
        )
    else:
        print("No fetch-depth:0 checkout steps carry the short-timeout risk signature.")

    return 0


if __name__ == "__main__":
    sys.exit(main())
