#!/usr/bin/env python3
"""Resolve a SonarCloud failure to file:line findings, with no SonarCloud session.

WHY(#6769): `SonarCloud Code Analysis` can fail a PR, and until now nobody working from
CI could read WHY. Every route anyone tried went through Sonar's own API, and every one
of them is closed to an anonymous caller. Re-measured against a live failing PR:

    qualitygates/project_status  -> {"errors":[{"msg":"Project doesn't exist"}]}
    hotspots/search              -> {"errors":[{"msg":"Project doesn't exist"}]}
    measures/component           -> {"errors":[{"msg":"Project doesn't exist"}]}
    issues/search                -> {"total":0, "issues":[]}

That last line is the dangerous one, and it is why this went unfixed through three
recurrences. `issues/search` does not 401 and does not error -- it returns a well-formed
EMPTY result. An agent that queries it concludes "no findings" and merges past a
security-rated failure, when the truthful answer is "you are not permitted to see them."
It fails open and quietly, which is indistinguishable from a clean bill.

The findings were reachable the whole time, by a route nobody tried: **SonarCloud posts
them as GitHub check-run annotations**, which the GitHub API serves to any caller with a
plain `GITHUB_TOKEN`. On the PR that prompted this, three annotations named exact files
and lines, and one of them was a real ReDoS in a regex over an attacker-supplied PR
title. The check was right and actionable every time; only the reading was broken.

So this reads GitHub, not Sonar. It needs no secret, no session, and no new access.
"""

from __future__ import annotations

import json
import logging
import os
import subprocess
import sys

LOGGER = logging.getLogger("sonar-findings")

REPO = os.environ.get("SONAR_FINDINGS_REPO", "forkwright/aletheia")
CHECK_NAME = "SonarCloud Code Analysis"

# A finding at this level is what fails the quality gate; the rest are advisory.
BLOCKING_LEVEL = "failure"


def gh_json(*args: str):
    result = subprocess.run(["gh", *args], capture_output=True, text=True, check=False)
    if result.returncode != 0:
        raise RuntimeError(f"gh {' '.join(args)} failed: {result.stderr.strip()}")
    return json.loads(result.stdout or "null")


def sonar_check_run(head_sha: str) -> dict | None:
    """The SonarCloud check run at `head_sha`, or None when it has not reported."""
    runs = gh_json(
        "api", f"repos/{REPO}/commits/{head_sha}/check-runs?per_page=100",
        "--jq", ".check_runs",
    ) or []
    matching = [run for run in runs if run.get("name") == CHECK_NAME]
    if not matching:
        return None
    # WHY the newest: a re-analysis leaves the superseded run in place, and reporting an
    # old run's findings against a new head is how a fixed PR keeps looking broken.
    return max(matching, key=lambda run: run.get("started_at") or "")


def annotations(check_run_id: int) -> list[dict]:
    return gh_json(
        "api", f"repos/{REPO}/check-runs/{check_run_id}/annotations", "--paginate"
    ) or []


def render(finding: dict) -> str:
    line = finding.get("start_line")
    where = f"{finding.get('path')}:{line}" if line else str(finding.get("path"))
    message = " ".join((finding.get("message") or "").split())
    return f"  {where}  [{finding.get('annotation_level')}] {finding.get('title')}\n      {message}"


def report(head_sha: str) -> int:
    run = sonar_check_run(head_sha)
    if run is None:
        LOGGER.info("sonar-findings: SonarCloud has not reported on %s", head_sha[:9])
        return 0

    conclusion = run.get("conclusion")
    found = annotations(run["id"])

    if conclusion not in {"failure", "action_required"}:
        LOGGER.info(
            "sonar-findings: SonarCloud %s on %s, %d annotation(s)",
            conclusion, head_sha[:9], len(found),
        )
        for finding in found:
            LOGGER.info("%s", render(finding))
        return 0

    LOGGER.error("sonar-findings: SonarCloud %s on %s", conclusion, head_sha[:9])
    summary = " ".join(((run.get("output") or {}).get("summary") or "").split())
    if summary:
        LOGGER.error("  gate: %s", summary[:400])

    if not found:
        # WHY this is loud rather than a pass: a failing gate with no annotations is
        # precisely the state that used to be indistinguishable from "clean", and
        # saying so is the whole point. Never let an empty read mean nothing is wrong.
        LOGGER.error("")
        LOGGER.error("  The gate FAILED and carries no annotations, so its findings")
        LOGGER.error("  are not retrievable from here. Do NOT read that as clean --")
        LOGGER.error("  Sonar's own API answers anonymous callers with an empty")
        LOGGER.error("  result rather than an error, which looks identical to a")
        LOGGER.error("  passing analysis. Open the dashboard link on the check run.")
        return 1

    blocking = [f for f in found if f.get("annotation_level") == BLOCKING_LEVEL]
    LOGGER.error("")
    LOGGER.error("  %d finding(s), %d at %s:", len(found), len(blocking), BLOCKING_LEVEL)
    for finding in found:
        LOGGER.error("%s", render(finding))
    return 1


def main() -> int:
    head_sha = os.environ.get("HEAD_SHA", "").strip()
    if not head_sha:
        LOGGER.error("sonar-findings: HEAD_SHA is required")
        return 2
    try:
        return report(head_sha)
    except RuntimeError:
        # WHY not a hard failure: this tool EXPLAINS another check's verdict. If it
        # cannot reach GitHub it must not invent a second red on a PR whose own state
        # is unknown to it -- that would be a check failing for a reason unrelated to
        # the code, the habit #6769 is about not starting.
        LOGGER.exception("sonar-findings: could not read the check run")
        return 0


if __name__ == "__main__":
    logging.basicConfig(format="%(message)s", level=logging.INFO, stream=sys.stderr)
    raise SystemExit(main())
