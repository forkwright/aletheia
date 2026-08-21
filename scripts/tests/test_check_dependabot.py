from __future__ import annotations

import datetime as dt
import importlib.util
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

SCRIPT_PATH = Path(__file__).resolve().parents[1] / "check-dependabot.py"
SPEC = importlib.util.spec_from_file_location("check_dependabot", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {SCRIPT_PATH}")
cd = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = cd
SPEC.loader.exec_module(cd)

NOW = dt.datetime(2026, 8, 21, tzinfo=dt.timezone.utc)
WEEKLY = {"package-ecosystem": "cargo", "schedule": {"interval": "weekly"}}


def pr(branch: str, days_ago: int, state: str = "MERGED") -> dict:
    created = (NOW - dt.timedelta(days=days_ago)).isoformat().replace("+00:00", "Z")
    return {"number": 1, "headRefName": branch, "createdAt": created, "state": state}


class BranchMatching(unittest.TestCase):
    def test_an_ecosystem_matches_its_branch_segment(self) -> None:
        prs = [pr("dependabot/cargo/serde-1.0.1", 1), pr("dependabot/github_actions/x", 1)]
        self.assertEqual(len(cd.for_ecosystem(prs, "cargo")), 1)

    def test_github_actions_matches_across_the_hyphen_underscore_difference(self) -> None:
        """WHY: the config says `github-actions`, the branch says `github_actions`.
        A literal comparison reports the busiest ecosystem as permanently dead."""
        prs = [pr("dependabot/github_actions/actions-checkout-5", 1)]
        self.assertEqual(len(cd.for_ecosystem(prs, "github-actions")), 1)

    def test_a_nested_directory_branch_still_matches_its_ecosystem(self) -> None:
        """WHY: a per-directory update names the path after the ecosystem --
        `dependabot/cargo/crates/theatron/proskenion/keyring-4.1.6`. Splitting on the
        whole branch rather than the second segment would miss every one of them, and
        those are exactly the roots #6828 added."""
        prs = [pr("dependabot/cargo/crates/theatron/proskenion/keyring-4.1.6", 1)]
        self.assertEqual(len(cd.for_ecosystem(prs, "cargo")), 1)


class Liveness(unittest.TestCase):
    def test_a_recent_pr_passes(self) -> None:
        ok, _ = cd.liveness(WEEKLY, [pr("dependabot/cargo/serde-1", 3)], NOW, None)
        self.assertTrue(ok)

    def test_silence_past_three_weekly_cycles_fails(self) -> None:
        """The defect: 113 days of nothing, indistinguishable from nothing to do."""
        ok, message = cd.liveness(WEEKLY, [pr("dependabot/cargo/serde-1", 113)], NOW, None)
        self.assertFalse(ok)
        self.assertIn("113 days", message)

    def test_one_missed_cycle_does_not_fire(self) -> None:
        """WHY: a check that fires on ordinary variance gets ignored, and then it is
        not there for the outage it was written for."""
        ok, _ = cd.liveness(WEEKLY, [pr("dependabot/cargo/serde-1", 10)], NOW, None)
        self.assertTrue(ok)

    def test_an_ecosystem_at_its_open_pr_limit_is_alive_not_silent(self) -> None:
        """WHY this passes: dependabot stops opening PRs once the limit is reached, so
        an unmerged queue produces the same silence. But those PRs are visible. Failing
        here would cry wolf about the one state that is NOT hidden, and a check people
        learn to ignore does not catch the state that is."""
        entry = dict(WEEKLY, **{"open-pull-requests-limit": 2})
        prs = [
            pr("dependabot/cargo/a-1", 200, state="OPEN"),
            pr("dependabot/cargo/b-1", 200, state="OPEN"),
        ]
        ok, message = cd.liveness(entry, prs, NOW, None)
        self.assertTrue(ok)
        self.assertIn("open-PR limit", message)

    def test_a_closed_pr_does_not_count_toward_the_limit(self) -> None:
        entry = dict(WEEKLY, **{"open-pull-requests-limit": 2})
        prs = [pr("dependabot/cargo/a-1", 200, state="CLOSED")] * 3
        ok, _ = cd.liveness(entry, prs, NOW, None)
        self.assertFalse(ok)

    def test_a_never_run_ecosystem_is_measured_from_the_config_edit(self) -> None:
        """WHY: adding an ecosystem would otherwise fail this check the day it lands."""
        ok, message = cd.liveness(WEEKLY, [], NOW, NOW - dt.timedelta(days=2))
        self.assertTrue(ok)
        self.assertIn("no PR has ever been opened", message)

    def test_a_never_run_ecosystem_that_is_long_overdue_fails(self) -> None:
        ok, _ = cd.liveness(WEEKLY, [], NOW, NOW - dt.timedelta(days=60))
        self.assertFalse(ok)

    def test_no_pr_and_no_history_says_so_rather_than_guessing(self) -> None:
        """WHY not a default baseline: on a shallow clone an invented one either passes
        or fails for a reason unrelated to dependabot, and the message would blame the
        ecosystem for the clone depth."""
        ok, message = cd.liveness(WEEKLY, [], NOW, None)
        self.assertFalse(ok)
        self.assertIn("full clone", message)

    def test_an_unknown_interval_is_an_error_not_a_default(self) -> None:
        """WHY: silently treating `quarterly` as weekly would make this check assert
        something the config does not say."""
        with self.assertRaises(SystemExit):
            cd.window_days({"schedule": {"interval": "quarterly"}})


class Coverage(unittest.TestCase):
    """The config's own WARNING says a missing row fails nothing. This is that failure."""

    def _repo(self, tmp: str, locks: list[str]) -> Path:
        root = Path(tmp)
        subprocess.run(["git", "init", "-q"], cwd=root, check=True)
        for lock in locks:
            path = root / lock
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("", encoding="utf-8")
        subprocess.run(["git", "add", "-A"], cwd=root, check=True)
        return root

    def test_every_lockfile_directory_is_a_resolution_root(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = self._repo(tmp, ["Cargo.lock", "fuzz/Cargo.lock"])
            self.assertEqual(cd.cargo_roots(root), ["/", "/fuzz"])

    def test_an_unlisted_resolution_root_fails(self) -> None:
        """The gap #6828 closed by hand: proskenion is the shipped desktop frontend, so
        the surface a root-only scan omitted was the one that reaches users."""
        with tempfile.TemporaryDirectory() as tmp:
            root = self._repo(
                tmp, ["Cargo.lock", "crates/theatron/proskenion/Cargo.lock"]
            )
            ok, message = cd.coverage({"directories": ["/"]}, root)
            self.assertFalse(ok)
            self.assertIn("/crates/theatron/proskenion", message)

    def test_a_fully_listed_config_passes(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = self._repo(tmp, ["Cargo.lock", "fuzz/Cargo.lock"])
            ok, _ = cd.coverage({"directories": ["/", "/fuzz"]}, root)
            self.assertTrue(ok)

    def test_an_untracked_lockfile_is_not_required(self) -> None:
        """WHY: dependabot reads the repository, not the working tree. Requiring a row
        for a gitignored lockfile would demand a directory it cannot resolve."""
        with tempfile.TemporaryDirectory() as tmp:
            root = self._repo(tmp, ["Cargo.lock"])
            (root / "scratch").mkdir()
            (root / "scratch" / "Cargo.lock").write_text("", encoding="utf-8")
            self.assertEqual(cd.cargo_roots(root), ["/"])

    def test_a_singular_directory_key_is_read_too(self) -> None:
        """WHY: the github-actions entry uses `directory`, and a cargo entry could
        regress to it -- which is the exact shape #6828 fixed."""
        with tempfile.TemporaryDirectory() as tmp:
            root = self._repo(tmp, ["Cargo.lock"])
            ok, _ = cd.coverage({"directory": "/"}, root)
            self.assertTrue(ok)


class ThisRepository(unittest.TestCase):
    def test_the_live_config_declares_both_ecosystems(self) -> None:
        """WHY one bound test: the logic above is generic, and a config that stopped
        declaring cargo would pass every other test in this file."""
        names = [e["package-ecosystem"] for e in cd.declared_ecosystems(cd.CONFIG)]
        self.assertIn("cargo", names)
        self.assertIn("github-actions", names)


if __name__ == "__main__":
    unittest.main()
