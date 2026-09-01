#!/usr/bin/env python3
"""Read-only reconciliation for tagged releases that did not publish exactly."""

from __future__ import annotations

import argparse
import datetime as dt
import importlib.util
import json
import os
import re
import subprocess
import sys
from pathlib import Path

MAX_PAGES = 20
DEFAULT_GRACE_HOURS = 12
TAG_RE = re.compile(
    r"^v(?P<major>\d+)\.(?P<minor>\d+)\.(?P<patch>\d+)"
    r"(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$"
)
CONTRACT_BASELINE = (0, 39, 0)


class HealthError(Exception):
    """The audit could not safely classify a release state."""


def _asset_checker() -> object:
    path = Path(__file__).with_name("check-release-assets.py")
    spec = importlib.util.spec_from_file_location("check_release_assets", path)
    if spec is None or spec.loader is None:
        raise HealthError(f"cannot load exact asset contract from {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


ASSETS = _asset_checker()


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


def tag_commit_date(repo: str, row: dict) -> str:
    name = row.get("name")
    commit = row.get("commit")
    sha = commit.get("sha") if isinstance(commit, dict) else None
    if not isinstance(name, str) or not isinstance(sha, str):
        raise HealthError("tags API returned an invalid tag identity")
    payload = gh_json(f"repos/{repo}/commits/{sha}")
    try:
        stamp = payload["commit"]["committer"]["date"]
    except (KeyError, TypeError):
        raise HealthError(f"commit API returned no date for {name}") from None
    if not isinstance(stamp, str):
        raise HealthError(f"commit API returned an invalid date for {name}")
    return stamp


def version(tag: str) -> tuple[int, int, int] | None:
    match = TAG_RE.fullmatch(tag)
    if match is None:
        return None
    return tuple(int(match.group(part)) for part in ("major", "minor", "patch"))


def in_contract(tag: str) -> bool:
    parsed = version(tag)
    return parsed is not None and parsed >= CONTRACT_BASELINE


def older_than(stamp: str | None, now: dt.datetime, grace_hours: int) -> bool:
    if stamp is None:
        return True
    try:
        then = dt.datetime.fromisoformat(stamp.replace("Z", "+00:00"))
    except ValueError:
        return True
    return now - then > dt.timedelta(hours=grace_hours)


def asset_names(assets: list[object]) -> set[str] | None:
    names: list[str] = []
    for asset in assets:
        if not isinstance(asset, dict) or not isinstance(asset.get("name"), str):
            return None
        names.append(asset["name"])
    return set(names) if len(set(names)) == len(names) else None


def asset_problem(release: dict, tag: str) -> str | None:
    assets = release.get("assets")
    if not isinstance(assets, list):
        return "release API returned an invalid asset inventory"
    draft = release.get("draft")
    if not isinstance(draft, bool):
        return "release API returned an invalid draft state"
    if draft:
        return f"still draft with {len(assets)} asset(s)"
    names = asset_names(assets)
    if names is None:
        return "release API returned an invalid asset inventory"
    if not names:
        return "published with zero assets"
    try:
        expected = ASSETS.expected_assets(tag)
    except ValueError as exc:
        return f"cannot evaluate the exact asset contract ({exc})"
    missing = sorted(expected - names)
    unexpected = sorted(names - expected)
    if missing or unexpected:
        pieces = []
        if missing:
            pieces.append(f"missing {', '.join(missing)}")
        if unexpected:
            pieces.append(f"unexpected {', '.join(unexpected)}")
        return f"published inventory is not exact ({'; '.join(pieces)})"
    return None


def violations(
    tag_rows: list[dict], releases: list[dict], now: dt.datetime, grace_hours: int
) -> list[str]:
    tags = {
        row.get("name"): row
        for row in tag_rows
        if isinstance(row.get("name"), str)
    }
    by_tag = {
        release.get("tag_name"): release
        for release in releases
        if isinstance(release.get("tag_name"), str)
    }
    failures: list[str] = []
    for tag, row in sorted(tags.items()):
        if not in_contract(tag):
            continue
        release = by_tag.get(tag)
        if release is None:
            commit = row.get("commit")
            stamp = commit.get("date") if isinstance(commit, dict) else None
            if older_than(stamp, now, grace_hours):
                failures.append(
                    f"{tag}: no release object (never created or deleted) past grace"
                )
            continue
        problem = asset_problem(release, tag)
        if problem and older_than(
            release.get("published_at") or release.get("created_at"), now, grace_hours
        ):
            failures.append(f"{tag}: {problem} past grace")
    for tag in sorted(by_tag):
        if in_contract(tag) and tag not in tags:
            failures.append(f"{tag}: release object exists but its tag is absent (tag deleted)")
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
    release_tags = {release.get("tag_name") for release in releases}
    for row in tag_rows:
        name = row.get("name")
        if isinstance(name, str) and in_contract(name) and name not in release_tags:
            row["commit"] = {"date": tag_commit_date(repo, row)}
    problems = violations(
        tag_rows, releases, dt.datetime.now(dt.timezone.utc), args.grace_hours
    )
    if problems:
        for problem in problems:
            print(f"release-health: {problem}", file=sys.stderr)
        print(f"::error title=release reconciliation::{problems[0]}")
        return 1
    checked = sum(in_contract(str(row.get("name", ""))) for row in tag_rows)
    print(f"release-health: {checked} in-contract tags have exact published inventories")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except HealthError as exc:
        print(f"release-health: API/evidence error: {exc}", file=sys.stderr)
        raise SystemExit(1)
