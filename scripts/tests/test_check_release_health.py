from __future__ import annotations

import datetime as dt
import importlib.util
import sys
import unittest
from pathlib import Path

from scripts.release_asset_inventory import expected_assets


def load(name: str, filename: str) -> object:
    spec = importlib.util.spec_from_file_location(name, Path(__file__).parents[1] / filename)
    if spec is None or spec.loader is None:
        raise RuntimeError(filename)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


health = load("check_release_health", "check-release-health.py")
NOW = dt.datetime(2026, 8, 31, tzinfo=dt.timezone.utc)
OLD = "2026-08-29T00:00:00Z"
FRESH = "2026-08-30T23:30:00Z"


def rows(tag: str) -> list[dict]:
    return [{"name": tag, "commit": {"date": OLD}}]


def release(tag: str, *, draft: bool = False, names: list[str] | None = None, when: str = OLD) -> dict:
    return {"tag_name": tag, "draft": draft, "assets": [{"name": name} for name in names or []], "created_at": when, "published_at": when}


class HealthTests(unittest.TestCase):
    tag = "v1.2.3-rc.1"

    def test_complete_prerelease_passes(self) -> None:
        complete = release(self.tag, names=sorted(expected_assets(self.tag)))
        self.assertEqual(health.violations(rows(self.tag), [complete], NOW, 12), [])

    def test_stale_draft_and_published_zero_assets_fail(self) -> None:
        self.assertIn("draft", health.violations(rows(self.tag), [release(self.tag, draft=True)], NOW, 12)[0])
        self.assertIn("zero assets", health.violations(rows(self.tag), [release(self.tag)], NOW, 12)[0])

    def test_missing_release_and_deleted_tag_are_distinct(self) -> None:
        self.assertIn("never created or deleted", health.violations(rows(self.tag), [], NOW, 12)[0])
        self.assertIn("tag deleted", health.violations([], [release(self.tag)], NOW, 12)[0])

    def test_fresh_draft_is_inside_grace(self) -> None:
        self.assertEqual(health.violations(rows(self.tag), [release(self.tag, draft=True, when=FRESH)], NOW, 12), [])

    def test_fresh_missing_release_is_inside_grace(self) -> None:
        fresh_rows = [{"name": self.tag, "commit": {"date": FRESH}}]
        self.assertEqual(health.violations(fresh_rows, [], NOW, 12), [])

    def test_incomplete_inventory_fails_and_rerun_is_idempotent(self) -> None:
        sample = [release(self.tag, names=["one"])]
        first = health.violations(rows(self.tag), sample, NOW, 12)
        self.assertEqual(first, health.violations(rows(self.tag), sample, NOW, 12))
        self.assertIn("not exact", first[0])

    def test_api_error_is_explicit(self) -> None:
        original = health.gh_json
        try:
            health.gh_json = lambda *args: (_ for _ in ()).throw(health.HealthError("denied"))
            with self.assertRaisesRegex(health.HealthError, "denied"):
                health.fetch_paged("owner/repo", "tags")
        finally:
            health.gh_json = original


if __name__ == "__main__":
    unittest.main()
