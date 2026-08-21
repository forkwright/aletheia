from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path
from unittest import mock

SCRIPT_PATH = Path(__file__).resolve().parents[1] / "release_pr_checks.py"
SPEC = importlib.util.spec_from_file_location("release_pr_checks", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {SCRIPT_PATH}")
rpc = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = rpc
SPEC.loader.exec_module(rpc)

RELEASE_PR = {
    "number": 6902,
    "headRefName": "release-please--branches--main",
    "headRefOid": "b2cc3a49d" + "0" * 31,
}


class BranchSelection(unittest.TestCase):
    def test_only_release_branches_are_considered(self) -> None:
        """WHY: this tool closes and re-runs checks. Aiming it at an ordinary PR would
        re-dispatch gates on work that already has a verdict."""
        listing = [
            RELEASE_PR,
            {"number": 1, "headRefName": "fix/6806-something", "headRefOid": "a" * 40},
        ]
        with mock.patch.object(rpc, "gh", return_value=__import__("json").dumps(listing)):
            found = rpc.open_release_prs()
        self.assertEqual([pr["number"] for pr in found], [6902])


class Healing(unittest.TestCase):
    def _heal_with(
        self, held: list[int], run_counts: dict[str, int]
    ) -> tuple[list[int], list[str]]:
        """Drive `heal` with a stub `gh`.

        `held` is the run ids GitHub reports as awaiting approval at the head;
        `run_counts` maps a required workflow to how many runs exist at that head.
        """
        approved: list[int] = []
        dispatched: list[str] = []

        def fake_gh(*args: str) -> str:
            if args[0] == "api" and args[1] == "-X":
                approved.append(int(args[3].split("/runs/")[1].split("/approve")[0]))
                return ""
            if args[0] == "api" and "/workflows/" in args[1] and "/runs?" in args[1]:
                workflow = args[1].split("/workflows/")[1].split("/runs")[0]
                return str(run_counts[workflow])
            if args[0] == "api" and "/actions/runs?" in args[1]:
                return __import__("json").dumps(held)
            if args[0] == "api" and "/workflows/" in args[1]:
                return "active"
            if args[0] == "workflow" and args[1] == "run":
                dispatched.append(args[2])
                return ""
            raise AssertionError(f"unexpected gh call: {args}")

        with mock.patch.object(rpc, "gh", side_effect=fake_gh):
            returned = rpc.heal(RELEASE_PR)
        self.assertEqual(returned, (approved, dispatched))
        return approved, dispatched

    def test_held_runs_are_approved(self) -> None:
        """The operation that actually unblocks the PR.

        Approving populates the PR's `statusCheckRollup`, which is the list branch
        protection reads. Measured on #6902: rollup 0 -> 32 on approving 25 held runs.
        """
        approved, dispatched = self._heal_with(
            held=[11, 22, 33],
            run_counts=dict.fromkeys(rpc.REQUIRED_CONTEXT_WORKFLOWS, 1),
        )
        self.assertEqual(approved, [11, 22, 33])
        self.assertEqual(dispatched, [])

    def test_a_held_run_is_approved_and_NOT_dispatched(self) -> None:
        """The inversion this correction turns on.

        The previous version treated a held run as absent and dispatched a second run
        beside it. That run's check runs attach to the COMMIT and never reach the PR's
        rollup, so the PR stayed blocked while the tool reported it healed. Held is a
        reason to approve; it was never a reason to dispatch.
        """
        approved, dispatched = self._heal_with(
            held=[7],
            run_counts=dict.fromkeys(rpc.REQUIRED_CONTEXT_WORKFLOWS, 1),
        )
        self.assertEqual(approved, [7])
        self.assertEqual(
            dispatched, [], "a run that exists must be approved, never duplicated"
        )

    def test_a_workflow_with_no_run_at_all_is_dispatched(self) -> None:
        """The one case dispatch still serves: there is nothing to approve."""
        counts = dict.fromkeys(rpc.REQUIRED_CONTEXT_WORKFLOWS, 1)
        counts["security.yml"] = 0
        approved, dispatched = self._heal_with(held=[], run_counts=counts)
        self.assertEqual(approved, [])
        self.assertEqual(dispatched, ["security.yml"])

    def test_nothing_to_do_is_a_no_op(self) -> None:
        """WHY idempotence matters: this runs on a schedule and on every regeneration."""
        approved, dispatched = self._heal_with(
            held=[], run_counts=dict.fromkeys(rpc.REQUIRED_CONTEXT_WORKFLOWS, 1)
        )
        self.assertEqual((approved, dispatched), ([], []))

    def test_a_workflow_that_cannot_be_dispatched_is_an_error(self) -> None:
        """WHY loud: a renamed or disabled workflow makes this tool silently stop
        healing -- absent rather than red, the same shape as the defect it fixes."""

        def fake_gh(*args: str) -> str:
            if args[0] == "api" and "/workflows/" in args[1] and "/runs?" in args[1]:
                return "0"
            if args[0] == "api" and "/actions/runs?" in args[1]:
                return "[]"
            if args[0] == "api":
                return "disabled_manually"
            raise AssertionError(f"unexpected gh call: {args}")

        with mock.patch.object(rpc, "gh", side_effect=fake_gh):
            with self.assertRaises(RuntimeError):
                rpc.heal(RELEASE_PR)


class Reporting(unittest.TestCase):
    def test_no_open_release_pr_is_success(self) -> None:
        with mock.patch.object(rpc, "open_release_prs", return_value=[]):
            self.assertEqual(rpc.main(), 0)

    def test_an_undispatchable_workflow_fails_the_job(self) -> None:
        with mock.patch.object(rpc, "open_release_prs", return_value=[RELEASE_PR]), \
             mock.patch.object(rpc, "rollup_size", return_value=5), \
             mock.patch.object(rpc, "heal", side_effect=RuntimeError("gone")):
            self.assertEqual(rpc.main(), 1)

    def test_an_empty_rollup_after_healing_is_a_FAILURE(self) -> None:
        """The correction, asserted.

        The previous version returned 0 for having dispatched something, whether or not
        the PR gained a single check. It therefore announced a repair it had not made.
        An empty rollup means branch protection still sees nothing, so the release is
        still stuck -- and that must be red, not green.
        """
        with mock.patch.object(rpc, "open_release_prs", return_value=[RELEASE_PR]), \
             mock.patch.object(rpc, "rollup_size", return_value=0), \
             mock.patch.object(rpc, "heal", return_value=([1, 2], ["security.yml"])):
            self.assertEqual(rpc.main(), 1)

    def test_a_populated_rollup_after_healing_is_success(self) -> None:
        with mock.patch.object(rpc, "open_release_prs", return_value=[RELEASE_PR]), \
             mock.patch.object(rpc, "rollup_size", side_effect=[0, 32]), \
             mock.patch.object(rpc, "heal", return_value=([1, 2], [])):
            self.assertEqual(rpc.main(), 0)

    def test_one_broken_pr_does_not_stop_the_others(self) -> None:
        """WHY: two repos cut releases from the same schedule. A sweep that aborted on
        the first problem would leave the second release blocked for another cycle."""
        other = dict(RELEASE_PR, number=7000)
        seen: list[int] = []

        def fake_heal(pr: dict[str, str]) -> tuple[list[int], list[str]]:
            seen.append(pr["number"])
            if pr["number"] == 6902:
                raise RuntimeError("gone")
            return ([], ["security.yml"])

        with mock.patch.object(rpc, "open_release_prs", return_value=[RELEASE_PR, other]), \
             mock.patch.object(rpc, "rollup_size", return_value=9), \
             mock.patch.object(rpc, "heal", side_effect=fake_heal):
            self.assertEqual(rpc.main(), 1)
        self.assertEqual(seen, [6902, 7000])


if __name__ == "__main__":
    unittest.main()
