from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT_PATH = Path(__file__).resolve().parents[1] / "check-automation-pr-gates.py"
SPEC = importlib.util.spec_from_file_location("check_automation_pr_gates", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {SCRIPT_PATH}")
ap = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = ap
SPEC.loader.exec_module(ap)


class WorkflowRunReferences(unittest.TestCase):
    """WHY(#6806): `workflow_run.workflows` matches a workflow's `name:` as a plain
    string. Rename the target and the trigger silently stops firing -- no error, no
    warning, just a workflow that never runs again. Absent rather than red, which is
    the shape the release-PR healer exists to fix in the first place."""

    def _dir(self, tmp: str, files: dict[str, str]) -> Path:
        d = Path(tmp)
        for name, body in files.items():
            (d / name).write_text(body, encoding="utf-8")
        return d

    def test_a_reference_and_its_target_are_found(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            d = self._dir(tmp, {
                "a.yml": "name: Target\non:\n  push:\n",
                "b.yml": 'name: Caller\non:\n  workflow_run:\n    workflows: ["Target"]\n',
            })
            self.assertEqual(ap.workflow_run_references(d), [("b.yml", "Target")])
            self.assertEqual(ap.declared_workflow_names(d), {"Target", "Caller"})

    def test_a_bare_on_key_is_still_read(self) -> None:
        """WHY pinned: PyYAML parses an unquoted `on:` as the BOOLEAN True, not the
        string "on". A reader that only consults data["on"] finds nothing in any
        workflow in this repository and reports a clean result forever."""
        with tempfile.TemporaryDirectory() as tmp:
            d = self._dir(tmp, {
                "b.yml": 'name: Caller\non:\n  workflow_run:\n    workflows: ["Target"]\n',
            })
            self.assertEqual(ap.workflow_run_references(d), [("b.yml", "Target")])

    def test_a_workflow_with_no_workflow_run_trigger_is_ignored(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            d = self._dir(tmp, {"a.yml": "name: Plain\non:\n  push:\n"})
            self.assertEqual(ap.workflow_run_references(d), [])

    def test_unparseable_yaml_does_not_abort_the_scan(self) -> None:
        """WHY: yaml-validate owns malformed YAML. Raising here would report a parse
        error as a broken workflow_run reference, which sends the reader to the wrong
        file."""
        with tempfile.TemporaryDirectory() as tmp:
            d = self._dir(tmp, {
                "bad.yml": "{not: yaml: at all",
                "b.yml": 'name: Caller\non:\n  workflow_run:\n    workflows: ["Target"]\n',
            })
            self.assertEqual(ap.workflow_run_references(d), [("b.yml", "Target")])

    def test_this_repository_resolves_every_reference(self) -> None:
        """WHY bound to the repo: the tests above are generic, and a reader that had
        stopped matching anything would pass every one of them."""
        d = ap.ROOT / ".github" / "workflows"
        declared = ap.declared_workflow_names(d)
        refs = ap.workflow_run_references(d)
        self.assertTrue(
            refs, "this repo does use workflow_run; an empty result means a broken reader"
        )
        for referrer, referenced in refs:
            self.assertIn(referenced, declared, f"{referrer} references {referenced!r}")


if __name__ == "__main__":
    unittest.main()
