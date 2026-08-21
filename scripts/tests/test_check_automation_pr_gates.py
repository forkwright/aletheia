from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path

SCRIPT_PATH = Path(__file__).resolve().parents[1] / "check-automation-pr-gates.py"
SPEC = importlib.util.spec_from_file_location("check_automation_pr_gates", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {SCRIPT_PATH}")
ap = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = ap
SPEC.loader.exec_module(ap)

HEADER = "# a comment\nsonar.sources=.\nsonar.tests=scripts\n"


def properties(exclusions: str, inclusions: str) -> str:
    return f"{HEADER}sonar.exclusions={exclusions}\nsonar.test.inclusions={inclusions}\n"


class SonarScopeRewrite(unittest.TestCase):
    """WHY this exists: the inventory is already derived by `test_fixture_paths`, and
    the check printed the correct list in its failure while refusing to write it. Every
    new test file then cost a hand transcription of a 28-entry comma-joined list."""

    def test_a_stale_inventory_is_rewritten_to_the_derived_one(self) -> None:
        out = ap.rewritten_sonar_scope(properties("old.py", "old.py"), ["a.py", "b.py"])
        self.assertIsNotNone(out)
        self.assertIn("sonar.exclusions=a.py,b.py\n", out or "")
        self.assertIn("sonar.test.inclusions=a.py,b.py\n", out or "")

    def test_both_keys_are_written_so_the_scopes_stay_disjoint(self) -> None:
        """WHY both: the check requires exclusions to MIRROR test.inclusions. Writing
        one would trade one failure for the other."""
        out = ap.rewritten_sonar_scope(properties("stale.py", "also-stale.py"), ["x.py"])
        lines = (out or "").splitlines()
        self.assertIn("sonar.exclusions=x.py", lines)
        self.assertIn("sonar.test.inclusions=x.py", lines)

    def test_an_already_correct_file_reports_no_change(self) -> None:
        self.assertIsNone(ap.rewritten_sonar_scope(properties("x.py", "x.py"), ["x.py"]))

    def test_the_other_keys_and_comments_survive(self) -> None:
        """WHY: the check also requires sonar.sources and sonar.tests to be exactly
        `.` and `scripts`. A rewrite that dropped them would pass its own fix and fail
        the check it was written to satisfy."""
        out = ap.rewritten_sonar_scope(properties("old.py", "old.py"), ["x.py"]) or ""
        self.assertIn("# a comment\n", out)
        self.assertIn("sonar.sources=.\n", out)
        self.assertIn("sonar.tests=scripts\n", out)

    def test_the_scope_keys_are_named_once(self) -> None:
        """WHY pinned: these two strings appeared four times each, and SonarCloud was
        right that a fifth spelling would diverge silently. This fails if someone
        reintroduces a literal instead of using the constant."""
        source = SCRIPT_PATH.read_text(encoding="utf-8")
        for key in ap.SONAR_SCOPE_KEYS:
            self.assertLessEqual(
                source.count(f'"{key}"'),
                1,
                f"{key} should be spelled once, in SONAR_SCOPE_KEYS",
            )


if __name__ == "__main__":
    unittest.main()
