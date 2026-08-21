from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

SCRIPT_PATH = Path(__file__).resolve().parents[1] / "check-automation-pr-gates.py"
SPEC = importlib.util.spec_from_file_location("check_automation_pr_gates", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {SCRIPT_PATH}")
ap = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = ap
SPEC.loader.exec_module(ap)

HEADER = "# a comment\nsonar.sources=.\nsonar.tests=scripts\n"


class SonarScopeRewrite(unittest.TestCase):
    """WHY this mode exists: the inventory is already derived by `test_fixture_paths`,
    and the check printed the correct 27-entry list in its failure while refusing to
    write it. Every new test file then cost a hand transcription of that list."""

    def _properties(self, tmp: str, exclusions: str, inclusions: str) -> Path:
        path = Path(tmp) / ".sonarcloud.properties"
        path.write_text(
            f"{HEADER}sonar.exclusions={exclusions}\nsonar.test.inclusions={inclusions}\n",
            encoding="utf-8",
        )
        return path

    def test_a_stale_inventory_is_rewritten_to_the_derived_one(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = self._properties(tmp, "scripts/old.py", "scripts/old.py")
            with mock.patch.object(ap, "test_fixture_paths", lambda: ["a.py", "b.py"]):
                self.assertTrue(ap.rewrite_sonar_test_scope(path))
            text = path.read_text(encoding="utf-8")
            self.assertIn("sonar.exclusions=a.py,b.py\n", text)
            self.assertIn("sonar.test.inclusions=a.py,b.py\n", text)

    def test_both_keys_are_written_so_the_scopes_stay_disjoint(self) -> None:
        """WHY both: the check requires exclusions to MIRROR test.inclusions. Writing
        one would trade one failure for the other."""
        with tempfile.TemporaryDirectory() as tmp:
            path = self._properties(tmp, "stale.py", "also-stale.py")
            with mock.patch.object(ap, "test_fixture_paths", lambda: ["x.py"]):
                ap.rewrite_sonar_test_scope(path)
            lines = path.read_text(encoding="utf-8").splitlines()
            self.assertIn("sonar.exclusions=x.py", lines)
            self.assertIn("sonar.test.inclusions=x.py", lines)

    def test_an_already_correct_file_is_reported_unchanged(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = self._properties(tmp, "x.py", "x.py")
            with mock.patch.object(ap, "test_fixture_paths", lambda: ["x.py"]):
                self.assertFalse(ap.rewrite_sonar_test_scope(path))

    def test_the_other_keys_and_comments_survive(self) -> None:
        """WHY: the check also requires sonar.sources and sonar.tests to be exactly
        `.` and `scripts`. A rewrite that dropped them would pass its own fix and fail
        the check it was written to satisfy."""
        with tempfile.TemporaryDirectory() as tmp:
            path = self._properties(tmp, "old.py", "old.py")
            with mock.patch.object(ap, "test_fixture_paths", lambda: ["x.py"]):
                ap.rewrite_sonar_test_scope(path)
            text = path.read_text(encoding="utf-8")
            self.assertIn("# a comment\n", text)
            self.assertIn("sonar.sources=.\n", text)
            self.assertIn("sonar.tests=scripts\n", text)


if __name__ == "__main__":
    unittest.main()
