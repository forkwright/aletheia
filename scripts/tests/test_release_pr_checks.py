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
    def _heal_with(self, run_states: dict[str, list[str]]) -> list[str]:
        """Drive `heal` with a stub `gh`, returning the workflows it dispatched.

        `run_states` maps a workflow to the run states GitHub reports at the head --
        e.g. `["success"]`, `["action_required"]`, or `[]` for none at all.
        """
        dispatched: list[str] = []

        def fake_gh(*args: str) -> str:
            if args[0] == "api" and "/runs?" in args[1]:
                workflow = args[1].split("/workflows/")[1].split("/runs")[0]
                return __import__("json").dumps(run_states[workflow])
            if args[0] == "api":
                return "active"
            if args[0] == "workflow" and args[1] == "run":
                dispatched.append(args[2])
                return ""
            raise AssertionError(f"unexpected gh call: {args}")

        with mock.patch.object(rpc, "gh", side_effect=fake_gh):
            returned = rpc.heal(RELEASE_PR)
        self.assertEqual(returned, dispatched)
        return dispatched

    def test_a_release_pr_with_no_runs_gets_every_workflow_dispatched(self) -> None:
        """The whole point: the PR arrived with its required contexts absent."""
        dispatched = self._heal_with(dict.fromkeys(rpc.REQUIRED_CONTEXT_WORKFLOWS, []))
        self.assertEqual(dispatched, list(rpc.REQUIRED_CONTEXT_WORKFLOWS))

    def test_a_release_pr_that_already_has_runs_is_left_alone(self) -> None:
        """WHY it matters that this is idempotent: it runs on a schedule. Dispatching
        every tick would replace a finished verdict with a pending one forever."""
        dispatched = self._heal_with(
            dict.fromkeys(rpc.REQUIRED_CONTEXT_WORKFLOWS, ["success"])
        )
        self.assertEqual(dispatched, [])

    def test_only_the_missing_workflow_is_dispatched(self) -> None:
        states = dict.fromkeys(rpc.REQUIRED_CONTEXT_WORKFLOWS, ["success"])
        states["security.yml"] = []
        dispatched = self._heal_with(states)
        self.assertEqual(dispatched, ["security.yml"])

    def test_a_run_HELD_FOR_APPROVAL_is_not_a_verdict(self) -> None:
        """The defect this whole file previously encoded. Release-please PRs get their
        runs CREATED and then held awaiting approval, and GitHub reports no check for a
        held run -- so asking "does a run exist" answered yes while the PR stayed
        blocked. Measured on #6902: 25 held runs at the head, one visible check."""
        dispatched = self._heal_with(
            dict.fromkeys(rpc.REQUIRED_CONTEXT_WORKFLOWS, ["action_required"])
        )
        self.assertEqual(dispatched, list(rpc.REQUIRED_CONTEXT_WORKFLOWS))

    def test_a_held_run_beside_a_real_one_still_counts_as_a_verdict(self) -> None:
        """WHY: a superseded generation leaves held runs behind. If ANY run at the head
        is doing or has done something, the checks are on their way and dispatching
        would only add noise."""
        dispatched = self._heal_with(
            dict.fromkeys(rpc.REQUIRED_CONTEXT_WORKFLOWS, ["action_required", "in_progress"])
        )
        self.assertEqual(dispatched, [])

    def test_a_failed_run_counts_as_a_verdict(self) -> None:
        """WHY not success-only: a red gate is a real verdict. Replacing it with a fresh
        pending run would make a failing release look unfinished, which is the exact
        confusion this tool exists to remove."""
        with mock.patch.object(rpc, "gh", return_value='["failure"]'):
            self.assertTrue(rpc.has_verdict_for("gate-attestation.yml", "a" * 40))

    def test_no_runs_at_all_is_not_a_verdict(self) -> None:
        with mock.patch.object(rpc, "gh", return_value="[]"):
            self.assertFalse(rpc.has_verdict_for("gate-attestation.yml", "a" * 40))

    def test_a_workflow_that_cannot_be_dispatched_is_an_error(self) -> None:
        """WHY loud: a renamed or disabled workflow makes this tool silently stop
        healing -- absent rather than red, the same shape as the defect it fixes."""

        def fake_gh(*args: str) -> str:
            if args[0] == "api" and "/runs?" in args[1]:
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
             mock.patch.object(rpc, "heal", side_effect=RuntimeError("gone")):
            self.assertEqual(rpc.main(), 1)

    def test_one_broken_pr_does_not_stop_the_others(self) -> None:
        """WHY: two repos cut releases from the same schedule. A sweep that aborted on
        the first problem would leave the second release blocked for another cycle."""
        other = dict(RELEASE_PR, number=7000)
        seen: list[int] = []

        def fake_heal(pr: dict[str, str]) -> list[str]:
            seen.append(pr["number"])
            if pr["number"] == 6902:
                raise RuntimeError("gone")
            return ["security.yml"]

        with mock.patch.object(rpc, "open_release_prs", return_value=[RELEASE_PR, other]), \
             mock.patch.object(rpc, "heal", side_effect=fake_heal):
            self.assertEqual(rpc.main(), 1)
        self.assertEqual(seen, [6902, 7000])


if __name__ == "__main__":
    unittest.main()
