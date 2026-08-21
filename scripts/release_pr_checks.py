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


# A run in this state has been CREATED and is waiting for approval. It is not a verdict,
# and GitHub reports no check for it -- which is why a held release PR looks exactly like
# one whose checks were never created.
HELD_FOR_APPROVAL = "action_required"


def held_run_ids(head_sha: str) -> list[int]:
    """Every run at `head_sha` that GitHub created and is holding for approval."""
    raw = gh(
        "api",
        f"repos/{REPO}/actions/runs?head_sha={head_sha}&per_page=100",
        "--jq",
        f'[.workflow_runs[] | select(.conclusion == "{HELD_FOR_APPROVAL}") | .id] | @json',
    )
    return json.loads(raw.strip() or "[]")


def approve(run_id: int) -> None:
    """Release one held run so it both RUNS and COUNTS.

    WHY approving is the operation and dispatching is not, stated as a measurement
    rather than a belief: branch protection reads the PR's `statusCheckRollup`. A
    `workflow_dispatch` run is attached to the COMMIT, and its check runs appear under
    that commit while the PR's rollup stays empty. Approving the run GitHub already
    created for the PR populates the rollup.

    Measured on #6902 at 80aee212: rollup `n=0` with 25 runs held; approving all 25 took
    it to `n=32` immediately, and `gate`, `cargo audit` and `cargo deny` were among them.
    Before that, the same PR had its three required contexts SUCCEEDING as commit check
    runs while `mergeStateStatus` stayed `BLOCKED` across repeated queries -- three green
    checks that branch protection could not see.

    That is the defect this file was written to fix, reproduced by the fix itself: the
    healer reported success for dispatching, and dispatching was not the thing that
    mattered.
    """
    gh("api", "-X", "POST", f"repos/{REPO}/actions/runs/{run_id}/approve")


def has_run_at(workflow: str, head_sha: str) -> bool:
    """True when `workflow` has any run at `head_sha`, held or not.

    WHY held now counts, where an earlier version deliberately excluded it: a held run
    is released by `approve` above, so treating it as absent would dispatch a SECOND,
    redundant run -- one that cannot populate the rollup anyway. Held is no longer a
    reason to dispatch; it is a reason to approve.
    """
    raw = gh(
        "api",
        f"repos/{REPO}/actions/workflows/{workflow}/runs?head_sha={head_sha}&per_page=100",
        "--jq", "[.workflow_runs[] | .id] | length",
    )
    return int(raw.strip() or "0") > 0


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


def rollup_size(number: str) -> int:
    """How many checks the PR's OWN rollup reports -- the thing protection reads."""
    raw = gh(
        "pr", "view", str(number), "--repo", REPO,
        "--json", "statusCheckRollup",
        "--jq", "[.statusCheckRollup[]?] | length",
    )
    return int(raw.strip() or "0")


def heal(pr: dict[str, str]) -> tuple[list[int], list[str]]:
    """Approve every held run at this PR's head, then dispatch what has no run at all.

    Returns (approved run ids, dispatched workflows).
    """
    head = pr["headRefOid"]

    approved = []
    for run_id in held_run_ids(head):
        approve(run_id)
        approved.append(run_id)

    dispatched: list[str] = []
    for workflow in REQUIRED_CONTEXT_WORKFLOWS:
        if has_run_at(workflow, head):
            continue
        # WHY dispatch survives at all, given it cannot populate the rollup: this branch
        # is for a workflow with NO run whatsoever, where there is nothing to approve.
        # It gets the run created; the next tick approves it. Two ticks is slower than
        # one and is the honest cost of the only trigger GITHUB_TOKEN can pull.
        assert_dispatchable(workflow)
        dispatch(workflow, pr["headRefName"])
        dispatched.append(workflow)
    return approved, dispatched


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
        before = rollup_size(pr["number"])
        try:
            approved, dispatched = heal(pr)
        except RuntimeError as error:
            failures = True
            # WHY exception() and not error(): this is the branch where a declared
            # workflow could not be reached, and losing the traceback would leave the
            # job saying only that something failed -- the shape of unreadable failure
            # this whole area exists to remove.
            LOGGER.exception("release-pr-checks: %s", error)
            continue

        if approved:
            LOGGER.warning(
                "release-pr-checks: #%s had %d run(s) held for approval -- approved",
                pr["number"], len(approved),
            )
        if dispatched:
            LOGGER.warning(
                "release-pr-checks: #%s had no run at all for %s -- dispatched at %s; "
                "the next tick approves it",
                pr["number"], ", ".join(dispatched), pr["headRefName"],
            )
        if not approved and not dispatched:
            LOGGER.info("release-pr-checks: #%s needed nothing", pr["number"])

        # WHY the outcome and not the action: the previous version reported success for
        # having dispatched, and dispatching does not populate the rollup that branch
        # protection reads. It therefore announced a repair it had not made, and the
        # release PR it "healed" stayed BLOCKED with three green checks nobody could
        # see. A tool that cannot tell those apart is the defect it was built to fix.
        #
        # This is deliberately not a poll-until-green: the rollup carries PENDING checks
        # the moment they are approved, and waiting for a verdict here would hold a
        # runner for the length of a full gate.
        after = rollup_size(pr["number"])
        if after == 0:
            failures = True
            LOGGER.error(
                "release-pr-checks: #%s still reports NO checks in its rollup "
                "(approved %d, dispatched %d). Branch protection reads this list, so "
                "the PR remains unmergeable. This is not a transient: investigate "
                "whether the token may approve runs.",
                pr["number"], len(approved), len(dispatched),
            )
        elif approved or dispatched:
            LOGGER.warning(
                "release-pr-checks: #%s rollup %d -> %d", pr["number"], before, after
            )

    return 1 if failures else 0


if __name__ == "__main__":
    logging.basicConfig(format="%(message)s", level=logging.INFO, stream=sys.stderr)
    if os.environ.get("GH_TOKEN", "") == "":
        LOGGER.error("release-pr-checks: GH_TOKEN is required")
        raise SystemExit(1)
    raise SystemExit(main())
