from __future__ import annotations

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT_PATH = Path(__file__).resolve().parents[1] / "check-pr-title-conventional.py"
SPEC = importlib.util.spec_from_file_location("check_pr_title_conventional", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {SCRIPT_PATH}")
ct = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = ct
SPEC.loader.exec_module(ct)

TYPES = ["feat", "fix", "perf", "refactor", "docs", "test", "chore", "ci", "style"]


class AcceptedTypes(unittest.TestCase):
    """Driven over a written config, so these test the derivation, not this repo's list."""

    def _config(self, tmp: str, sections: list[dict[str, object]]) -> Path:
        path = Path(tmp) / "release-please-config.json"
        path.write_text(json.dumps({"changelog-sections": sections}), encoding="utf-8")
        return path

    def test_types_come_from_the_config(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = self._config(tmp, [{"type": "feat"}, {"type": "wibble"}])
            self.assertEqual(ct.accepted_types(path), ["feat", "wibble"])

    def test_a_hidden_section_still_counts(self) -> None:
        """WHY: hidden means "do not print this section", not "cannot parse this type".
        Excluding them would reject `chore(main): release 0.40.0` -- the title
        release-please writes for its own PR."""
        with tempfile.TemporaryDirectory() as tmp:
            path = self._config(tmp, [{"type": "chore", "hidden": True}])
            self.assertEqual(ct.accepted_types(path), ["chore"])

    def test_a_config_with_no_sections_is_an_error_not_an_empty_allowlist(self) -> None:
        """WHY: an empty list would reject every title, and the message would blame
        each PR rather than the config this check reads."""
        with tempfile.TemporaryDirectory() as tmp:
            path = self._config(tmp, [])
            with self.assertRaises(SystemExit):
                ct.accepted_types(path)


class TitleShape(unittest.TestCase):
    def ok(self, title: str) -> None:
        self.assertIsNone(ct.violation(title, TYPES), f"should accept: {title}")

    def bad(self, title: str) -> str:
        problem = ct.violation(title, TYPES)
        self.assertIsNotNone(problem, f"should reject: {title}")
        return problem or ""

    def test_the_shapes_this_repo_actually_merges(self) -> None:
        self.ok("fix(recall): the Semantic Scholar api-key rode along across redirects")
        self.ok("feat(ci): bound the population of outbound requests")
        self.ok("chore(main): release 0.40.0")
        self.ok("docs: say which side of the increment is returned")
        self.ok("feat(organon)!: drop the legacy tool schema")
        self.ok("fix!: refuse a lease whose digest does not bind")

    def test_a_prose_title_is_rejected(self) -> None:
        """The defect: five of these merged in one day and none reached the changelog."""
        self.bad("Query budgets and cancellation reasons, plus a retry primitive")

    def test_a_capitalised_type_names_the_actual_problem(self) -> None:
        """WHY a distinct message: the generic one reads as though the whole shape were
        wrong, and the author re-writes a title that was one character from correct."""
        problem = self.bad("Fix(recall): something")
        self.assertIn("lowercase", problem)

    def test_a_space_before_the_colon_is_rejected_and_explained(self) -> None:
        problem = self.bad("fix (recall): something")
        self.assertIn("lowercase", problem)

    def test_a_type_outside_the_config_is_named(self) -> None:
        problem = self.bad("build: bump the toolchain")
        self.assertIn("build", problem)
        self.assertIn("release-please-config.json", problem)

    def test_an_empty_description_is_rejected(self) -> None:
        self.bad("fix: ")

    def test_an_empty_title_is_rejected(self) -> None:
        self.bad("")

    def test_a_scope_containing_a_slash_is_accepted(self) -> None:
        """WHY: `pylon/skene` is a scope this repo already uses."""
        self.ok("fix(pylon/skene): approval events cross sessions")


class ThisRepositoryParses(unittest.TestCase):
    def test_the_live_config_yields_the_types_in_use(self) -> None:
        """WHY one test bound to this repo: the derivation above is generic, and a
        config that stopped declaring `feat` would pass every test yet reject the
        titles that actually carry releases."""
        types = ct.accepted_types(ct.CONFIG)
        for expected in ("feat", "fix", "chore"):
            self.assertIn(expected, types)


if __name__ == "__main__":
    unittest.main()
