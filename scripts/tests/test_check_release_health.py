from __future__ import annotations

import datetime as dt
import importlib.util
import sys
import unittest
from pathlib import Path
from unittest import mock


def load(name: str, filename: str) -> object:
    spec = importlib.util.spec_from_file_location(name, Path(__file__).parents[1] / filename)
    if spec is None or spec.loader is None:
        raise RuntimeError(filename)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


inventory = load("release_asset_inventory", "release_asset_inventory.py")
health = load("check_release_health", "check-release-health.py")
NOW = dt.datetime(2026, 8, 31, tzinfo=dt.timezone.utc)
OLD = "2026-08-29T00:00:00Z"
FRESH = "2026-08-30T23:30:00Z"


def rows(tag: str) -> list[dict]:
    return [{"name": tag}]


def release(
    tag: str,
    *,
    draft: bool = False,
    names: list[str] | None = None,
    created_at: str = OLD,
    updated_at: str | None = OLD,
    published_at: str | None = None,
) -> dict:
    return {
        "tag_name": tag,
        "draft": draft,
        "assets": [{"name": name} for name in names or []],
        # GitHub returns this as the target commit timestamp, not release age.
        "created_at": created_at,
        "updated_at": updated_at,
        "published_at": published_at if draft else (published_at or OLD),
    }


class HealthTests(unittest.TestCase):
    tag = "v1.2.3-rc.1"

    def test_complete_prerelease_passes(self) -> None:
        complete = release(self.tag, names=sorted(inventory.expected_assets(self.tag)))
        self.assertEqual(health.violations(rows(self.tag), [complete], NOW, 12), [])

    def test_stale_draft_and_published_zero_assets_fail(self) -> None:
        self.assertIn("draft", health.violations(rows(self.tag), [release(self.tag, draft=True)], NOW, 12)[0])
        self.assertIn("zero assets", health.violations(rows(self.tag), [release(self.tag)], NOW, 12)[0])

    def test_unreadable_release_is_explicitly_ambiguous(self) -> None:
        failures, ambiguities = health.reconciliation(rows(self.tag), [], {}, NOW, 12)
        self.assertEqual(failures, [])
        self.assertIn("no readable published release", ambiguities[0])
        self.assertIn("indistinguishable", ambiguities[0])

    def test_orphaned_release_is_not_called_a_deleted_tag(self) -> None:
        self.assertEqual(health.violations([], [release(self.tag)], NOW, 12), [])

    def test_fresh_draft_on_old_commit_is_inside_update_grace(self) -> None:
        fresh = release(
            self.tag, draft=True, created_at=OLD, updated_at=FRESH, published_at=None
        )
        self.assertEqual(health.violations(rows(self.tag), [fresh], NOW, 12), [])

    def test_draft_with_no_release_activity_clock_is_ambiguous(self) -> None:
        unreadable_clock = release(self.tag, draft=True, updated_at=None)
        failures, ambiguities = health.reconciliation(
            rows(self.tag), [unreadable_clock], {}, NOW, 12
        )
        self.assertEqual(failures, [])
        self.assertIn("no usable release-update timestamp", ambiguities[0])

    def test_old_commit_cannot_make_an_unreadable_release_stale(self) -> None:
        old_commit = [{"name": self.tag, "commit": {"date": OLD}}]
        failures, ambiguities = health.reconciliation(old_commit, [], {}, NOW, 12)
        self.assertEqual(failures, [])
        self.assertNotIn("past grace", ambiguities[0])

    def test_incomplete_inventory_fails_and_rerun_is_idempotent(self) -> None:
        sample = [release(self.tag, names=["one"])]
        first = health.violations(rows(self.tag), sample, NOW, 12)
        self.assertEqual(first, health.violations(rows(self.tag), sample, NOW, 12))
        self.assertIn("not exact", first[0])

    def test_duplicate_releases_are_rejected_deterministically(self) -> None:
        duplicate = [release(self.tag), release(self.tag, names=["one"])]
        self.assertIn("multiple readable release objects", health.violations(rows(self.tag), duplicate, NOW, 12)[0])

    def test_missing_tag_for_old_published_release_fails_after_publication_grace(self) -> None:
        failures, ambiguities = health.reconciliation(
            [], [release(self.tag)], {self.tag: False}, NOW, 12
        )
        self.assertIn("currently missing tag ref", failures[0])
        self.assertEqual(ambiguities, [])

    def test_missing_tag_for_fresh_published_release_is_within_grace(self) -> None:
        fresh = release(self.tag, published_at=FRESH)
        failures, ambiguities = health.reconciliation(
            [], [fresh], {self.tag: False}, NOW, 12
        )
        self.assertEqual(failures, [])
        self.assertIn("inside publication grace", ambiguities[0])

    def test_tag_ref_appearing_after_paged_snapshot_is_not_called_deleted(self) -> None:
        failures, ambiguities = health.reconciliation(
            [], [release(self.tag)], {self.tag: True}, NOW, 12
        )
        self.assertEqual((failures, ambiguities), ([], []))

    def test_non_404_tag_ref_error_is_explicit(self) -> None:
        with mock.patch.object(health.subprocess, "run") as run:
            run.return_value = mock.Mock(returncode=1, stderr="HTTP 500")
            with self.assertRaisesRegex(health.HealthError, "tag-ref"):
                health.tag_ref_exists("owner/repo", self.tag)

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
