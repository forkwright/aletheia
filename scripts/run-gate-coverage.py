#!/usr/bin/env python3
"""Run the `gate-coverage-scripts` CI job's checks locally.

These checks gate every PR through the required `gate` context, are pure Python
and shell, and finish in seconds -- but there was no way to run them except by
pushing and waiting for CI. That put a multi-minute round trip in front of a
one-second answer, on the checks most likely to be tripped by an ordinary edit.

The step list is DERIVED from .github/workflows/gate-attestation.yml, never
restated here. A local runner carrying its own copy of the list is a second
source of truth that silently drifts from the job it claims to reproduce, and
the drift only shows up as a CI failure the local run said would not happen --
the exact thing this exists to prevent.

Nothing is skipped silently. A step this runner cannot faithfully reproduce is
reported as UNRUN and makes the whole run fail, because a runner that quietly
omits part of its job is worse than no runner: it produces a green that means
less than it appears to. This job's own history is the argument -- #6523 records
it reporting success on every PR without scanning anything, because a missing
ripgrep went unnoticed.

Usage:
    scripts/run-gate-coverage.py [--job JOB] [--list]
"""

from __future__ import annotations

import argparse
import pathlib
import re
import subprocess
import sys

import yaml

REPO_ROOT = pathlib.Path(__file__).resolve().parent.parent
WORKFLOW = REPO_ROOT / ".github" / "workflows" / "gate-attestation.yml"
DEFAULT_JOB = "gate-coverage-scripts"

# WHY a substitution rule rather than an expression evaluator: exactly one
# expression appears in this job, and guessing at the rest would be the same
# restatement this script exists to avoid. `github.base_ref` is the PR's base
# branch, whose local analogue is the tracked base a working branch is compared
# against. Anything else is reported UNRUN rather than guessed.
EXPRESSION = re.compile(r"\$\{\{(.+?)\}\}", re.S)


def resolve_expression(expr: str) -> str | None:
    return "origin/main" if "github.base_ref" in expr else None


def substitute(run: str) -> tuple[str, list[str]]:
    """Return the runnable command plus any expressions that could not resolve."""
    unresolved: list[str] = []

    def repl(match: re.Match[str]) -> str:
        value = resolve_expression(match.group(1))
        if value is None:
            unresolved.append(match.group(1).strip())
            return match.group(0)
        return value

    return EXPRESSION.sub(repl, run), unresolved


# WHY apt lines are dropped rather than run: the CI runner installs tools a
# developer box already has, and `sudo apt-get install` from a local check would
# be an unpleasant surprise. What is kept is the REST of the step -- which for
# the ripgrep step is the workflow's own `command -v rg` assertion, written
# there precisely because this job once reported success without scanning
# anything (#6523). Running the step's own verification preserves the guarantee
# without this script guessing a package-to-binary mapping: the package is
# `ripgrep` and the binary is `rg`, and a guess would have failed on exactly
# that.
APT_LINE = re.compile(r"^.*\bapt-get\b.*$", re.M)


def strip_apt(run: str) -> tuple[str, bool]:
    """Drop apt-get lines, keeping whatever verification the step also carries."""
    if not APT_LINE.search(run):
        return run, False
    remainder = APT_LINE.sub("", run).strip()
    return remainder, True


def load_steps(job: str) -> list[tuple[str, str]]:
    data = yaml.safe_load(WORKFLOW.read_text())
    jobs = data.get("jobs", {})
    if job not in jobs:
        sys.exit(f"{WORKFLOW.name} has no job {job!r}; jobs are: {', '.join(sorted(jobs))}")
    steps = []
    for step in jobs[job].get("steps", []):
        run = step.get("run")
        if run:
            steps.append((step.get("name") or run.strip().splitlines()[0][:40], run))
    return steps


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--job", default=DEFAULT_JOB, help=f"workflow job to reproduce (default: {DEFAULT_JOB})")
    ap.add_argument("--list", action="store_true", help="list the derived steps without running them")
    args = ap.parse_args()

    steps = load_steps(args.job)
    if args.list:
        for name, _ in steps:
            print(f"  {name}")
        return 0

    failed: list[str] = []
    unrun: list[str] = []
    for name, run in steps:
        body, had_apt = strip_apt(run)
        if had_apt and not body:
            # An install step with no verification of its own: there is nothing
            # faithful left to run, and inventing a check here would be a guess.
            unrun.append(f"{name}: install-only step with no assertion to reproduce")
            print(f"  UNRUN {name}")
            continue
        suffix = "  (apt lines dropped; running its own assertion)" if had_apt else ""

        command, unresolved = substitute(body)
        if unresolved:
            unrun.append(f"{name}: unresolved expression(s): {'; '.join(unresolved)}")
            print(f"  UNRUN {name}")
            continue

        proc = subprocess.run(
            ["bash", "-e", "-c", command],
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
            check=False,
        )
        if proc.returncode == 0:
            print(f"  PASS  {name}{suffix}")
        else:
            failed.append(name)
            print(f"  FAIL  {name}")
            for line in (proc.stdout + proc.stderr).strip().splitlines()[-12:]:
                print(f"          {line}")

    print()
    if unrun:
        print(f"{len(unrun)} step(s) could NOT be reproduced locally:")
        for item in unrun:
            print(f"  - {item}")
        print("Teach this runner to resolve them, or run them in CI -- do not treat this as green.")
    if failed:
        print(f"{len(failed)} step(s) failed:")
        for item in failed:
            print(f"  - {item}")
    if failed or unrun:
        return 1
    print(f"{args.job}: all {len(steps)} steps passed locally.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
