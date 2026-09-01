#!/usr/bin/env python3
"""Make a release run name an unpublished or incomplete tagged release."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
from pathlib import Path

from release_asset_inventory import expected_assets, release_inventory_problem

MAX_PAGES = 20
PASSING_CONCLUSIONS = frozenset({"success", "skipped"})


class OutcomeError(Exception):
    """The reporter could not obtain evidence for an outcome."""


def gh_json(*args: str) -> object:
    try:
        result = subprocess.run(
            ["gh", "api", *args], capture_output=True, text=True, check=True
        )
        return json.loads(result.stdout)
    except (subprocess.CalledProcessError, json.JSONDecodeError) as exc:
        detail = getattr(exc, "stderr", "") or str(exc)
        raise OutcomeError(f"GitHub API request failed: {detail.strip()}") from exc


def fetch_run_jobs(repo: str, run_id: str) -> list[dict]:
    jobs: list[dict] = []
    for page in range(1, MAX_PAGES + 1):
        payload = gh_json(
            f"repos/{repo}/actions/runs/{run_id}/jobs?filter=latest&per_page=100&page={page}"
        )
        if not isinstance(payload, dict) or not isinstance(payload.get("jobs"), list):
            raise OutcomeError("GitHub API returned an invalid jobs response")
        batch = payload["jobs"]
        jobs.extend(job for job in batch if isinstance(job, dict))
        if len(batch) < 100:
            return jobs
    raise OutcomeError(f"run {run_id} has more than {MAX_PAGES * 100} jobs")


def fetch_release(repo: str, tag: str) -> dict | None:
    """Find a release including drafts; the per-tag endpoint hides drafts."""
    for page in range(1, MAX_PAGES + 1):
        payload = gh_json(f"repos/{repo}/releases?per_page=100&page={page}")
        if not isinstance(payload, list):
            raise OutcomeError("GitHub API returned an invalid releases response")
        for release in payload:
            if isinstance(release, dict) and release.get("tag_name") == tag:
                return release
        if len(payload) < 100:
            return None
    raise OutcomeError(f"release list exceeds {MAX_PAGES * 100} entries")


def asset_problem(release: dict | None, tag: str) -> str | None:
    if release is None:
        return f"{tag}: no release object (never created or deleted)"
    problem = release_inventory_problem(release, tag)
    return f"{tag}: {problem}" if problem else None


def problem_jobs(jobs: list[dict]) -> list[str]:
    problems: list[str] = []
    for job in jobs:
        conclusion = job.get("conclusion")
        if conclusion is None or conclusion in PASSING_CONCLUSIONS:
            continue
        problems.append(f"{job.get('name', 'unnamed job')}: {conclusion}")
    return problems


def evaluate(jobs: list[dict], release: dict | None, tag: str) -> list[str]:
    release_problem = asset_problem(release, tag)
    if release_problem is None:
        return []
    return [release_problem, *(f"cause: {job}" for job in problem_jobs(jobs))]


def evaluate_with_retries(
    jobs: list[dict], repo: str, tag: str, attempts: int, retry_seconds: int
) -> list[str]:
    problems: list[str] = []
    for attempt in range(attempts):
        problems = evaluate(jobs, fetch_release(repo, tag), tag)
        if not problems:
            return []
        if attempt + 1 < attempts:
            time.sleep(retry_seconds)
    return problems


def write_summary(lines: list[str]) -> None:
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if not summary:
        return
    with Path(summary).open("a", encoding="utf-8") as handle:
        handle.write("## Release outcome\n\n")
        handle.write("\n".join(f"- {line}" for line in lines))
        handle.write("\n")


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--attempts", type=int, default=1)
    parser.add_argument("--retry-seconds", type=int, default=0)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    if args.attempts < 1 or args.retry_seconds < 0:
        raise OutcomeError("attempts must be positive and retry-seconds non-negative")
    repo = os.environ["GH_REPO"]
    tag = os.environ["RELEASE_TAG"]
    run_id = os.environ["RUN_ID"]
    jobs = fetch_run_jobs(repo, run_id)
    if not jobs:
        raise OutcomeError(f"run {run_id} listed no jobs; outcome cannot be certified")
    problems = evaluate_with_retries(jobs, repo, tag, args.attempts, args.retry_seconds)
    if not problems:
        success = f"{tag}: published with the exact expected asset inventory"
        print(f"release-outcome: {success}")
        write_summary([success])
        return 0
    for problem in problems:
        print(f"release-outcome: {problem}", file=sys.stderr)
    write_summary(problems)
    print(f"::error title=release outcome::{problems[0]}")
    return 1


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except OutcomeError as exc:
        print(f"release-outcome: API/evidence error: {exc}", file=sys.stderr)
        write_summary([f"API/evidence error: {exc}"])
        raise SystemExit(1)
