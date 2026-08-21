#!/usr/bin/env python3
"""Start the required checks on a release PR that never got any.

WHY(#6806): release-please creates its PR with `GITHUB_TOKEN`, and GitHub does not
raise workflow-triggering events for anything that token does. The PR therefore
arrives with its required contexts *absent* rather than red -- and branch protection
holds a PR with a missing context forever, with nothing to re-run and nothing to
approve. Measured across two repos in one day: five release PRs, every one of them,
none recoverable without a human noticing and closing/reopening by hand.

The failure is silent in the worst direction. A red check advertises itself; a missing
check looks exactly like a PR that has not finished yet. Releases stop, and the only
symptom is a PR that seems to be waiting.

`workflow_dispatch` is the one `GITHUB_TOKEN`-created event GitHub deliberately allows
to start another workflow -- this repo already depends on that in release-please.yml,
to dispatch release.yml at the new tag. Dispatching the gating workflows at the release
branch produces check runs on that branch's head SHA, which is the PR's head SHA, which
is what branch protection reads.

Runs from two triggers, deliberately:

  * from release-please.yml, the moment the PR is created -- the root fix, closing the
    window rather than waiting for a tick;
  * from a schedule -- because the root fix can regress, and a scheduled sweep is the
    only form that still works when it does.

One implementation for both, so the two cannot drift.

WHY dispatch rather than approving the held run: approving keeps the checks in their
own PR context and is what `kanon ci_hold_sweep` does, but it is not established that a
workflow's own `GITHUB_TOKEN` may call the approve endpoint, and a self-heal that fails
silently on permissions would be the same defect again. Dispatch is verified to work --
`workflow_dispatch` is not subject to the bot-PR approval gate, and release-please.yml
already depends on that. A dispatched run attaches its check runs to the same head SHA,
which is what branch protection reads.
"""

from __future__ import annotations

import json
import logging
import os
import subprocess
import sys

LOGGER = logging.getLogger("release-pr-checks")

REPO = "forkwright/aletheia"

# Release-please names its branch from the config; every release PR carries this prefix.
RELEASE_BRANCH_PREFIX = "release-please--branches--"

# The workflows that produce the branch-protection required contexts
# (`gate`, `cargo audit`, `cargo deny`).
#
# WHY declared rather than derived from branch protection: reading
# `/branches/{b}/protection` needs admin, which no workflow token has here. The
# restatement is guarded instead of trusted -- `assert_dispatchable` fails when a named
# workflow is missing or has lost its `workflow_dispatch` trigger, which is the drift
# that would otherwise turn this whole check into a no-op nobody notices.
REQUIRED_CONTEXT_WORKFLOWS = (
    "gate-attestation.yml",
    "security.yml",
)


def gh(*args: str) -> str:
    """Run `gh` and return stdout, raising with stderr attached on failure."""
    result = subprocess.run(
        ["gh", *args], capture_output=True, text=True, check=False
    )
    if result.returncode != 0:
        raise RuntimeError(f"gh {' '.join(args)} failed: {result.stderr.strip()}")
    return result.stdout


def open_release_prs() -> list[dict[str, str]]:
    """Open PRs whose head branch is a release-please branch."""
    raw = gh(
        "pr", "list", "--repo", REPO, "--state", "open", "--limit", "50",
        "--json", "number,headRefName,headRefOid",
    )
    return [
        pr
        for pr in json.loads(raw)
        if pr["headRefName"].startswith(RELEASE_BRANCH_PREFIX)
    ]


# A run in this state has been CREATED and is waiting for a human to approve it. It is
# not a verdict, and GitHub does not report it as a check -- which is why a held release
# PR looks exactly like one whose checks were never created.
HELD_FOR_APPROVAL = "action_required"


def has_verdict_for(workflow: str, head_sha: str) -> bool:
    """True when `workflow` has a run at `head_sha` that is doing or has done something.

    WHY not simply "a run exists": the first version of this asked exactly that, and it
    was a no-op against the failure it was written for. Release-please PRs get their runs
    CREATED and then held in `action_required` awaiting approval. A held run exists, so
    "does a run exist" answered yes, so nothing was dispatched -- while the PR stayed
    blocked, because GitHub reports no check for a held run. Measured on #6902: 25 held
    runs at the current head, 75 more across superseded heads, and one check visible.

    `action_required` is the absence of a verdict wearing a `conclusion` field.

    WHY success and failure both count: a run that FAILED is a real verdict, and
    re-dispatching would replace it with a fresh pending one -- turning a red release
    into one that looks unfinished. That reasoning was right and is unchanged.
    """
    raw = gh(
        "api",
        f"repos/{REPO}/actions/workflows/{workflow}/runs?head_sha={head_sha}&per_page=100",
        "--jq", "[.workflow_runs[] | .conclusion // .status] | @json",
    )
    states = json.loads(raw.strip() or "[]")
    return any(state != HELD_FOR_APPROVAL for state in states)


def assert_dispatchable(workflow: str) -> None:
    """Fail when a declared workflow cannot be dispatched.

    WHY this is an error and not a skip: a workflow that has lost its
    `workflow_dispatch` trigger, or been renamed, makes this tool silently stop doing
    the one thing it does. That is the same shape as the defect it was written for --
    a check that is absent rather than red.
    """
    raw = gh(
        "api", f"repos/{REPO}/actions/workflows/{workflow}",
        "--jq", ".state",
    )
    if raw.strip() != "active":
        raise RuntimeError(f"{workflow} is not active: {raw.strip()!r}")


def dispatch(workflow: str, ref: str) -> None:
    gh("workflow", "run", workflow, "--repo", REPO, "--ref", ref)


def heal(pr: dict[str, str]) -> list[str]:
    """Dispatch every required-context workflow with no run at this PR's head.

    Returns the workflows dispatched, empty when the PR already has its checks.
    """
    dispatched: list[str] = []
    for workflow in REQUIRED_CONTEXT_WORKFLOWS:
        if has_verdict_for(workflow, pr["headRefOid"]):
            continue
        assert_dispatchable(workflow)
        dispatch(workflow, pr["headRefName"])
        dispatched.append(workflow)
    return dispatched


def main() -> int:
    prs = open_release_prs()
    if not prs:
        LOGGER.info("release-pr-checks: no open release PR")
        return 0

    failures = False
    for pr in prs:
        LOGGER.info(
            "release-pr-checks: #%s at %s", pr["number"], pr["headRefOid"][:9]
        )
        try:
            dispatched = heal(pr)
        except RuntimeError as error:
            failures = True
            # WHY exception() and not error(): this is the branch where a declared
            # workflow could not be reached, and losing the traceback would leave the
            # job saying only that something failed -- the shape of unreadable failure
            # this whole area exists to remove.
            LOGGER.exception("release-pr-checks: %s", error)
            continue
        if dispatched:
            LOGGER.warning(
                "release-pr-checks: #%s had no run for %s -- dispatched at %s",
                pr["number"],
                ", ".join(dispatched),
                pr["headRefName"],
            )
        else:
            LOGGER.info("release-pr-checks: #%s already has its checks", pr["number"])

    return 1 if failures else 0


if __name__ == "__main__":
    logging.basicConfig(format="%(message)s", level=logging.INFO, stream=sys.stderr)
    if os.environ.get("GH_TOKEN", "") == "":
        LOGGER.error("release-pr-checks: GH_TOKEN is required")
        raise SystemExit(1)
    raise SystemExit(main())
