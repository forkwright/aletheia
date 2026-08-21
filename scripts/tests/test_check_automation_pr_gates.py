from __future__ import annotations

import importlib.util
import sys
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


class DerivedSonarScope(unittest.TestCase):
    """WHY this exists: the inventory is already derived by `test_fixture_paths`, and
    the check printed the correct list in its failure while leaving a human to retype it
    into two comma-joined properties. Every new test file cost that transcription."""

    def test_both_keys_are_emitted_so_the_scopes_stay_disjoint(self) -> None:
        """WHY both: the check requires exclusions to MIRROR test.inclusions. Emitting
        one would trade one failure for the other."""
        lines = ap.derived_sonar_scope(["a.py", "b.py"]).splitlines()
        self.assertEqual(
            lines, ["sonar.exclusions=a.py,b.py", "sonar.test.inclusions=a.py,b.py"]
        )

    def test_the_emitted_lines_are_what_the_check_demands(self) -> None:
        """The property that matters: pasting this output satisfies the check. Driven
        through the live inventory, so it fails if the two ever diverge in shape."""
        emitted = dict(
            line.split("=", 1) for line in ap.derived_sonar_scope(ap.test_fixture_paths()).splitlines()
        )
        expected = ",".join(ap.test_fixture_paths())
        for key in ap.SONAR_SCOPE_KEYS:
            self.assertEqual(emitted[key], expected)

    def test_it_writes_nothing(self) -> None:
        """WHY asserted: the first version rewrote `.sonarcloud.properties` in place,
        which meant building a target path from this file\'s own location -- flagged as
        path construction from uncontrolled data, and a larger thing to trust than two
        lines on stdout. This fails if a writer comes back."""
        with mock.patch.object(Path, "write_text", side_effect=AssertionError("no writes")):
            ap.derived_sonar_scope(["a.py"])

    def test_every_scope_key_is_named_once(self) -> None:
        """WHY pinned: these four strings appeared three or four times each, and
        SonarCloud was right that a further spelling would diverge silently. It caught
        two I had missed while fixing the first two."""
        source = SCRIPT_PATH.read_text(encoding="utf-8")
        keys = (*ap.SONAR_SCOPE_KEYS, ap.SONAR_SOURCES_KEY, ap.SONAR_TESTS_KEY)
        for key in keys:
            self.assertLessEqual(
                source.count(f'"{key}"'), 1, f"{key} should be spelled once, as a constant"
            )


if __name__ == "__main__":
    unittest.main()
