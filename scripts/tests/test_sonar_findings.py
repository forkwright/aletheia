from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path
from unittest import mock

SCRIPT_PATH = Path(__file__).resolve().parents[1] / "sonar-findings.py"
SPEC = importlib.util.spec_from_file_location("sonar_findings", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {SCRIPT_PATH}")
sf = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = sf
SPEC.loader.exec_module(sf)

SHA = "9e69e490c70d2fcf729a8c6248d2d8a3a0339dd2"

# Verbatim shape of the three annotations SonarCloud posted on the PR that prompted
# this, including the ReDoS that failed the security rating.
REAL_ANNOTATIONS = [
    {
        "path": ".github/workflows/pr-title.yml",
        "start_line": 14,
        "annotation_level": "warning",
        "title": "Move this read permission from workflow level to job level.",
        "message": "See more on https://sonarcloud.io/project/issues?id=forkwright_aletheia",
    },
    {
        "path": "scripts/release_pr_checks.py",
        "start_line": 149,
        "annotation_level": "failure",
        "title": 'Use "logging.exception()" instead.',
        "message": "See more on https://sonarcloud.io/project/issues?id=forkwright_aletheia",
    },
    {
        "path": "scripts/check-pr-title-conventional.py",
        "start_line": 69,
        "annotation_level": "warning",
        "title": "Simplify this regular expression to reduce its runtime, as it has "
        "super-linear performance due to backtracking.",
        "message": "See more on https://sonarcloud.io/project/issues?id=forkwright_aletheia",
    },
]


def run(name: str, conclusion: str, started: str = "2026-08-21T11:00:00Z", run_id: int = 1):
    return {
        "id": run_id,
        "name": name,
        "conclusion": conclusion,
        "started_at": started,
        "output": {"summary": "Quality Gate failed"},
    }


class RunSelection(unittest.TestCase):
    def test_the_sonar_run_is_picked_out_of_every_check_on_the_commit(self) -> None:
        runs = [run("gate", "success"), run(sf.CHECK_NAME, "failure", run_id=7)]
        with mock.patch.object(sf, "gh_json", return_value=runs):
            self.assertEqual((sf.sonar_check_run(SHA) or {})["id"], 7)

    def test_the_newest_run_wins_when_a_reanalysis_superseded_one(self) -> None:
        """WHY: a superseded run stays on the commit, and reporting its findings makes
        a PR that was already fixed keep looking broken -- the same
        stale-run confusion `gh pr checks` produces elsewhere in this repo."""
        runs = [
            run(sf.CHECK_NAME, "failure", started="2026-08-21T10:00:00Z", run_id=1),
            run(sf.CHECK_NAME, "success", started="2026-08-21T12:00:00Z", run_id=2),
        ]
        with mock.patch.object(sf, "gh_json", return_value=runs):
            self.assertEqual((sf.sonar_check_run(SHA) or {})["id"], 2)

    def test_no_sonar_run_is_not_an_error(self) -> None:
        """WHY: SonarCloud reports later than the fast checks, and a tool that reddened
        a PR for arriving early would be worse than the silence it replaces."""
        with mock.patch.object(sf, "gh_json", return_value=[run("gate", "success")]):
            self.assertIsNone(sf.sonar_check_run(SHA))


class Reporting(unittest.TestCase):
    def _report(self, check_run, found):
        with mock.patch.object(sf, "sonar_check_run", return_value=check_run), \
             mock.patch.object(sf, "annotations", return_value=found):
            return sf.report(SHA)

    def test_a_failing_gate_with_annotations_is_reported_and_fails(self) -> None:
        self.assertEqual(self._report(run(sf.CHECK_NAME, "failure"), REAL_ANNOTATIONS), 1)

    def test_every_annotation_renders_a_file_and_line(self) -> None:
        """This IS the `Done when:`: a security-rated failure resolved to specific
        file:line findings by a caller with no interactive session."""
        rendered = [sf.render(a) for a in REAL_ANNOTATIONS]
        self.assertIn("scripts/check-pr-title-conventional.py:69", rendered[2])
        self.assertIn("super-linear", rendered[2])
        self.assertIn("scripts/release_pr_checks.py:149", rendered[1])

    def test_a_failing_gate_with_NO_annotations_still_fails_and_says_why(self) -> None:
        """WHY the loudest branch: an empty read is exactly what Sonar's own API returns
        to an anonymous caller -- `{"total":0}`, no error. Treating empty as clean is
        how a security-rated failure gets merged past, three times."""
        self.assertEqual(self._report(run(sf.CHECK_NAME, "failure"), []), 1)

    def test_a_passing_gate_does_not_fail_even_with_advisory_annotations(self) -> None:
        self.assertEqual(self._report(run(sf.CHECK_NAME, "success"), REAL_ANNOTATIONS), 0)

    def test_no_sonar_report_yet_is_a_pass(self) -> None:
        self.assertEqual(self._report(None, []), 0)

    def test_action_required_counts_as_a_failure(self) -> None:
        self.assertEqual(self._report(run(sf.CHECK_NAME, "action_required"), []), 1)

    def test_a_github_outage_does_not_invent_a_second_red(self) -> None:
        """WHY it passes: this tool EXPLAINS another check's verdict. Failing because it
        could not reach GitHub would put a red on a PR for a reason unrelated to its
        code -- the habit #6769 exists to avoid starting."""
        with mock.patch.dict("os.environ", {"HEAD_SHA": SHA}), \
             mock.patch.object(sf, "report", side_effect=RuntimeError("gh down")):
            self.assertEqual(sf.main(), 0)

    def test_a_missing_head_sha_is_an_error_not_a_silent_pass(self) -> None:
        with mock.patch.dict("os.environ", {"HEAD_SHA": ""}):
            self.assertEqual(sf.main(), 2)


if __name__ == "__main__":
    unittest.main()
