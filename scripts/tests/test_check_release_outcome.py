from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path


def load(name: str, filename: str) -> object:
    spec = importlib.util.spec_from_file_location(name, Path(__file__).parents[1] / filename)
    if spec is None or spec.loader is None:
        raise RuntimeError(filename)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


outcome = load("check_release_outcome", "check-release-outcome.py")


def release(*, draft: bool = False, names: list[str] | None = None) -> dict:
    return {"draft": draft, "assets": [{"name": name} for name in names or []]}


class OutcomeTests(unittest.TestCase):
    tag = "v1.2.3"

    def test_complete_release_passes(self) -> None:
        names = sorted(outcome.ASSETS.expected_assets(self.tag))
        self.assertEqual(outcome.evaluate([], release(names=names), self.tag), [])

    def test_draft_names_tag_and_cause(self) -> None:
        result = outcome.evaluate(
            [{"name": "publish-release", "conclusion": "skipped"}, {"name": "build", "conclusion": "failure"}],
            release(draft=True), self.tag,
        )
        self.assertIn(self.tag, result[0])
        self.assertIn("draft", result[0])
        self.assertEqual(result[1], "cause: build: failure")

    def test_published_zero_assets_fails(self) -> None:
        self.assertIn("zero assets", outcome.asset_problem(release(), self.tag))

    def test_missing_release_is_not_an_api_error_or_draft(self) -> None:
        self.assertIn("never created or deleted", outcome.asset_problem(None, self.tag))

    def test_incomplete_inventory_fails_exactly(self) -> None:
        result = outcome.asset_problem(release(names=["one"]), self.tag)
        self.assertIn("not exact", result)
        self.assertIn("missing", result)

    def test_eventual_consistency_retry_can_recover(self) -> None:
        original_fetch = outcome.fetch_release
        original_sleep = outcome.time.sleep
        complete = release(names=sorted(outcome.ASSETS.expected_assets(self.tag)))
        responses = iter([release(draft=True), complete])
        try:
            outcome.fetch_release = lambda repo, tag: next(responses)
            outcome.time.sleep = lambda seconds: self.assertEqual(seconds, 0)
            self.assertEqual(outcome.evaluate_with_retries([], "owner/repo", self.tag, 2, 0), [])
        finally:
            outcome.fetch_release = original_fetch
            outcome.time.sleep = original_sleep

    def test_api_error_is_explicit(self) -> None:
        original = outcome.gh_json
        try:
            outcome.gh_json = lambda *args: (_ for _ in ()).throw(outcome.OutcomeError("denied"))
            with self.assertRaisesRegex(outcome.OutcomeError, "denied"):
                outcome.fetch_release("owner/repo", self.tag)
        finally:
            outcome.gh_json = original


if __name__ == "__main__":
    unittest.main()
