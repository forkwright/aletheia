#!/usr/bin/env python3
"""Read-only reconciliation for release tags and readable release snapshots.

The Releases API response available to this read-only workflow is not proof
that no draft exists: GitHub may hide drafts from callers without push access.
Likewise, the Tags API has no tag-creation time. An absent readable release is
therefore an explicit evidence-unknown failure, not an age/deletion claim.
Grace applies only to release activity timestamps, never target commit dates.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import re
import subprocess
import sys
from urllib.parse import quote

from release_asset_inventory import release_inventory_problem

MAX_PAGES = 20
MAX_TAG_REF_CHECKS = 100
DEFAULT_GRACE_HOURS = 12
TAG_RE = re.compile(
    r"^v(?P<major>\d+)\.(?P<minor>\d+)\.(?P<patch>\d+)"
    r"(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$"
)
CONTRACT_BASELINE = (0, 39, 0)


class HealthError(Exception):
    """The audit could not safely classify a release state."""


def gh_json(*args: str) -> object:
    try:
        result = subprocess.run(
            ["gh", "api", *args], capture_output=True, text=True, check=True
        )
        return json.loads(result.stdout)
    except (subprocess.CalledProcessError, json.JSONDecodeError) as exc:
        detail = getattr(exc, "stderr", "") or str(exc)
        raise HealthError(f"GitHub API request failed: {detail.strip()}") from exc


def fetch_paged(repo: str, resource: str) -> list[dict]:
    rows: list[dict] = []
    for page in range(1, MAX_PAGES + 1):
        payload = gh_json(f"repos/{repo}/{resource}?per_page=100&page={page}")
        if not isinstance(payload, list) or not all(
            isinstance(row, dict) for row in payload
        ):
            raise HealthError(f"invalid {resource} API response")
        rows.extend(payload)
        if len(payload) < 100:
            return rows
    raise HealthError(f"{resource} exceeds the {MAX_PAGES * 100}-row audit bound")


def version(tag: str) -> tuple[int, int, int] | None:
    match = TAG_RE.fullmatch(tag)
    if match is None:
        return None
    return tuple(int(match.group(part)) for part in ("major", "minor", "patch"))


def in_contract(tag: str) -> bool:
    parsed = version(tag)
    return parsed is not None and parsed >= CONTRACT_BASELINE


def older_than(stamp: str, now: dt.datetime, grace_hours: int) -> bool:
    try:
        then = dt.datetime.fromisoformat(stamp.replace("Z", "+00:00"))
    except ValueError:
        raise HealthError(f"invalid release activity timestamp: {stamp!r}") from None
    return now - then > dt.timedelta(hours=grace_hours)


def tag_state(
    tag: str,
    releases_by_tag: dict[str, dict],
    duplicate_tags: set[str],
    now: dt.datetime,
    grace_hours: int,
) -> tuple[str | None, str | None]:
    if tag in duplicate_tags:
        return (f"{tag}: multiple readable release objects make the snapshot ambiguous", None)
    release = releases_by_tag.get(tag)
    if release is None:
        return (None,
            f"{tag}: no readable published release; a hidden draft, deleted release, "
            "or never-released tag is indistinguishable with this read-only token"
        )
    problem = release_inventory_problem(release, tag)
    if problem is None:
        return (None, None)
    if release.get("draft") is True:
        updated_at = release.get("updated_at")
        if not isinstance(updated_at, str):
            return (None, f"{tag}: readable draft has no usable release-update timestamp")
        if older_than(updated_at, now, grace_hours):
            return (f"{tag}: draft inactive since its last release API update", None)
        return (None, None)
    published_at = release.get("published_at")
    if not isinstance(published_at, str):
        return (None, f"{tag}: readable published release has no usable publication timestamp")
    if older_than(published_at, now, grace_hours):
        return (f"{tag}: {problem} past publication grace", None)
    return (None, None)


def release_snapshot(releases: list[dict]) -> tuple[dict[str, dict], set[str]]:
    """Index one API snapshot without choosing an order-dependent duplicate."""
    by_tag: dict[str, dict] = {}
    duplicate_tags: set[str] = set()
    for release in releases:
        tag = release.get("tag_name")
        if not isinstance(tag, str):
            continue
        if tag in by_tag:
            duplicate_tags.add(tag)
        else:
            by_tag[tag] = release
    return by_tag, duplicate_tags


def tag_ref_exists(repo: str, tag: str) -> bool:
    """Read one current tag ref, treating only a verified 404 as absent."""
    result = subprocess.run(
        ["gh", "api", f"repos/{repo}/git/ref/tags/{quote(tag, safe='')}"],
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode == 0:
        return True
    if "HTTP 404" in result.stderr:
        return False
    raise HealthError(f"GitHub tag-ref request failed: {result.stderr.strip()}")


def orphan_published_tags(tags: set[str], releases: list[dict]) -> list[str]:
    """Find release rows requiring a second, current tag-ref snapshot."""
    candidates = {
        release.get("tag_name")
        for release in releases
        if isinstance(release.get("tag_name"), str)
        and in_contract(release["tag_name"])
        and release.get("draft") is False
        and release["tag_name"] not in tags
    }
    if len(candidates) > MAX_TAG_REF_CHECKS:
        raise HealthError(
            f"published release/tag reconciliation exceeds {MAX_TAG_REF_CHECKS}-ref bound"
        )
    return sorted(candidates)


def reconciliation(
    tag_rows: list[dict],
    releases: list[dict],
    tag_refs: dict[str, bool],
    now: dt.datetime,
    grace_hours: int,
) -> tuple[list[str], list[str]]:
    tags = {
        row.get("name"): row
        for row in tag_rows
        if isinstance(row.get("name"), str)
    }
    by_tag, duplicate_tags = release_snapshot(releases)
    failures: list[str] = []
    ambiguities: list[str] = []
    for tag in sorted(tags):
        if not in_contract(tag):
            continue
        failure, ambiguity = tag_state(
            tag, by_tag, duplicate_tags, now, grace_hours
        )
        if failure is not None:
            failures.append(failure)
        if ambiguity is not None:
            ambiguities.append(ambiguity)
    for tag in sorted(duplicate_tags - set(tags)):
        if in_contract(tag):
            failures.append(
                f"{tag}: multiple readable release objects make the snapshot ambiguous"
            )
    for tag in orphan_published_tags(set(tags), releases):
        ref_exists = tag_refs.get(tag)
        if ref_exists is True:
            continue  # The tag appeared after the paged snapshot.
        if ref_exists is None:
            raise HealthError(f"missing current tag-ref evidence for {tag}")
        release = by_tag.get(tag)
        if tag in duplicate_tags or release is None:
            continue
        published_at = release.get("published_at")
        if not isinstance(published_at, str):
            ambiguities.append(
                f"{tag}: readable published release has no usable publication timestamp "
                "for its currently missing tag ref"
            )
        elif older_than(published_at, now, grace_hours):
            failures.append(
                f"{tag}: readable published release has a currently missing tag ref "
                "past publication grace"
            )
        else:
            ambiguities.append(
                f"{tag}: readable published release has a currently missing tag ref "
                "inside publication grace"
            )
    return failures, ambiguities


def violations(
    tag_rows: list[dict], releases: list[dict], now: dt.datetime, grace_hours: int
) -> list[str]:
    """Compatibility wrapper for pure failure-only checks."""
    tags = {
        row.get("name") for row in tag_rows if isinstance(row.get("name"), str)
    }
    tag_refs = {tag: True for tag in orphan_published_tags(tags, releases)}
    failures, _ = reconciliation(tag_rows, releases, tag_refs, now, grace_hours)
    return failures


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--grace-hours", type=int, default=DEFAULT_GRACE_HOURS)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    if args.grace_hours < 0:
        raise HealthError("grace-hours must be non-negative")
    repo = os.environ.get("GH_REPO", "forkwright/aletheia")
    tag_rows = fetch_paged(repo, "tags")
    releases = fetch_paged(repo, "releases")
    tag_names = {
        row.get("name") for row in tag_rows if isinstance(row.get("name"), str)
    }
    tag_refs = {
        tag: tag_ref_exists(repo, tag)
        for tag in orphan_published_tags(tag_names, releases)
    }
    problems, ambiguities = reconciliation(
        tag_rows, releases, tag_refs, dt.datetime.now(dt.timezone.utc), args.grace_hours
    )
    if problems:
        for problem in problems:
            print(f"release-health: {problem}", file=sys.stderr)
        print(f"::error title=release reconciliation::{problems[0]}")
        return 1
    checked = sum(in_contract(str(row.get("name", ""))) for row in tag_rows)
    if ambiguities:
        for ambiguity in ambiguities:
            print(f"release-health: evidence unknown: {ambiguity}", file=sys.stderr)
        print(f"::error title=release reconciliation evidence unknown::{ambiguities[0]}")
        return 1
    print(f"release-health: {checked} in-contract tags have verified readable inventories")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except HealthError as exc:
        print(f"release-health: API/evidence error: {exc}", file=sys.stderr)
        raise SystemExit(1)
