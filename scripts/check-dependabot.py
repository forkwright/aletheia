#!/usr/bin/env python3
"""A dependabot ecosystem that stops producing looks exactly like one with nothing to do.

WHY(#6829): `.github/dependabot.yml` declares two ecosystems on the same weekly
schedule. `github-actions` ran every week. `cargo` opened its last PR on 2026-04-24 and
then nothing for **113 days**, across 135 direct crate dependencies, while the file sat
unchanged. Nothing detected it. There is no red check for a PR that was never opened,
and the absence of updates is indistinguishable from having none available.

It resolved when an unrelated edit to that same file made dependabot re-read the config
-- which is the worst way to learn a thing is fixed. Had nobody been editing the file
for another reason, it would still be silent. The root cause was never established, so
the only durable correction is to make the silence observable.

Two properties, both of them shaped by that file already carrying a comment which
describes a gap and cannot enforce it:

  LIVENESS  every declared ecosystem has produced a PR inside its own schedule's
            window, or is demonstrably blocked on us rather than stuck.

  COVERAGE  every independent cargo resolution root is listed under `directories`.
            The config's own WARNING says a new excluded workspace "needs a row here --
            nothing fails when one is missing, which is why both went unnoticed". This
            is that failure. A directory with its own `Cargo.lock` IS a separate
            resolution root: cargo writes a lockfile only at a workspace root, so a scan
            rooted at `/` resolves none of its dependencies.
"""

from __future__ import annotations

import datetime as dt
import json
import logging
import subprocess
import sys
from pathlib import Path

import yaml

LOGGER = logging.getLogger("check-dependabot")

REPO = "forkwright/aletheia"
REPO_ROOT = Path(__file__).resolve().parents[1]
CONFIG = REPO_ROOT / ".github" / "dependabot.yml"

# WHY three and not one: a single missed run is ordinary -- a holiday, a transient
# resolution failure, a rate limit -- and a check that fires on it gets ignored, which
# costs more than the delay it saves. Three consecutive misses is not noise; the outage
# this was written for was sixteen.
MISSED_CYCLES_BEFORE_ALARM = 3

INTERVAL_DAYS = {"daily": 1, "weekly": 7, "monthly": 30}

# Dependabot's default when an ecosystem does not declare one.
DEFAULT_OPEN_PR_LIMIT = 5


def normalize(ecosystem: str) -> str:
    """Dependabot writes `github-actions` into a branch as `github_actions`."""
    return ecosystem.replace("-", "_")


def gh_json(*args: str):
    result = subprocess.run(["gh", *args], capture_output=True, text=True, check=False)
    if result.returncode != 0:
        raise RuntimeError(f"gh {' '.join(args)} failed: {result.stderr.strip()}")
    return json.loads(result.stdout)


def declared_ecosystems(config_path: Path) -> list[dict]:
    config = yaml.safe_load(config_path.read_text(encoding="utf-8"))
    updates = config.get("updates") or []
    if not updates:
        raise SystemExit(f"{config_path.name} declares no updates; nothing to check")
    return updates


def window_days(entry: dict) -> int:
    interval = (entry.get("schedule") or {}).get("interval", "weekly")
    if interval not in INTERVAL_DAYS:
        raise SystemExit(
            f"unknown dependabot interval {interval!r}; add it to INTERVAL_DAYS "
            "rather than letting this check silently accept it"
        )
    return INTERVAL_DAYS[interval] * MISSED_CYCLES_BEFORE_ALARM


def config_touched_at() -> dt.datetime | None:
    """When dependabot.yml last changed.

    WHY this is the baseline when an ecosystem has never opened a PR: a config edit is
    what un-wedged the 113-day outage, so the clock for "should have produced something
    by now" starts at the last edit, not at the repository's creation. Without it, adding
    a new ecosystem would fail this check on the day it is added.
    """
    result = subprocess.run(
        ["git", "log", "-1", "--format=%cI", "--", str(CONFIG.relative_to(REPO_ROOT))],
        capture_output=True, text=True, check=False, cwd=REPO_ROOT,
    )
    stamp = result.stdout.strip()
    if result.returncode != 0 or not stamp:
        # A shallow clone can see no commit touching this file. Say so rather than
        # inventing a baseline -- a wrong one fails or passes for the wrong reason.
        return None
    return dt.datetime.fromisoformat(stamp)


def dependabot_prs() -> list[dict]:
    return gh_json(
        "pr", "list", "--repo", REPO, "--author", "app/dependabot",
        "--state", "all", "--limit", "200",
        "--json", "number,headRefName,createdAt,state",
    )


def for_ecosystem(prs: list[dict], ecosystem: str) -> list[dict]:
    """PRs whose branch names this ecosystem: `dependabot/<ecosystem>/...`."""
    wanted = normalize(ecosystem)
    matched = []
    for pr in prs:
        parts = pr["headRefName"].split("/")
        if len(parts) >= 2 and parts[0] == "dependabot" and normalize(parts[1]) == wanted:
            matched.append(pr)
    return matched


def liveness(entry: dict, prs: list[dict], now: dt.datetime, baseline: dt.datetime | None):
    """Return (ok, message) for one ecosystem."""
    ecosystem = entry["package-ecosystem"]
    mine = for_ecosystem(prs, ecosystem)
    allowed = window_days(entry)

    limit = entry.get("open-pull-requests-limit", DEFAULT_OPEN_PR_LIMIT)
    open_count = sum(1 for pr in mine if pr["state"] == "OPEN")
    if open_count >= limit:
        # Alive and blocked on us. This is a visible state -- the PRs are sitting
        # there -- so failing here would cry wolf about the one thing that is NOT
        # silent, and teach people to ignore the check that catches the thing that is.
        return True, (
            f"{ecosystem}: at its open-PR limit ({open_count}/{limit}); it will open "
            "no more until those are merged or closed"
        )

    dates = [dt.datetime.fromisoformat(pr["createdAt"].replace("Z", "+00:00")) for pr in mine]
    if dates:
        latest = max(dates)
        source = "last PR"
    elif baseline is not None:
        latest = baseline
        source = "last dependabot.yml edit (no PR has ever been opened)"
    else:
        return False, (
            f"{ecosystem}: no PR has ever been opened and dependabot.yml's history is "
            "not available, so this check cannot tell a new ecosystem from a dead one. "
            "Run it on a full clone."
        )

    age = (now - latest).days
    if age > allowed:
        return False, (
            f"{ecosystem}: {age} days since {source} ({latest.date()}), "
            f"past the {allowed}-day window "
            f"({MISSED_CYCLES_BEFORE_ALARM} missed {(entry.get('schedule') or {}).get('interval', 'weekly')} runs)"
        )
    return True, f"{ecosystem}: {age} days since {source}, inside the {allowed}-day window"


def cargo_roots(repo_root: Path) -> list[str]:
    """Every independent cargo resolution root, as a dependabot `directories` path.

    A tracked `Cargo.lock` marks one: cargo writes a lockfile only at a workspace root,
    so anything with its own lock is invisible to a scan rooted at `/`.
    """
    result = subprocess.run(
        ["git", "ls-files", "*Cargo.lock"],
        capture_output=True, text=True, check=False, cwd=repo_root,
    )
    if result.returncode != 0:
        raise SystemExit(f"git ls-files failed: {result.stderr.strip()}")
    roots = []
    for line in result.stdout.splitlines():
        parent = str(Path(line).parent)
        roots.append("/" if parent == "." else f"/{parent}")
    return sorted(set(roots))


def coverage(entry: dict, repo_root: Path):
    """Return (ok, message) for the cargo entry's directory coverage."""
    listed = set(entry.get("directories") or ([entry["directory"]] if "directory" in entry else []))
    actual = set(cargo_roots(repo_root))
    missing = sorted(actual - listed)
    if missing:
        return False, (
            "cargo: these have their own Cargo.lock and so are separate resolution "
            f"roots, but are not listed under `directories`: {', '.join(missing)}. "
            "A scan rooted at / resolves none of their dependencies."
        )
    return True, f"cargo: all {len(actual)} resolution root(s) listed"


def report(ok: bool, message: str) -> bool:
    """Log one result at the level its verdict deserves; return whether it failed."""
    (LOGGER.info if ok else LOGGER.error)("check-dependabot: %s", message)
    return not ok


def evaluate(entries: list[dict], prs: list[dict], now, baseline, coverage_only: bool) -> bool:
    """Run every applicable check over `entries`; return True when any failed."""
    failures = False
    for entry in entries:
        if not coverage_only:
            failures |= report(*liveness(entry, prs, now, baseline))
        if entry["package-ecosystem"] == "cargo":
            failures |= report(*coverage(entry, REPO_ROOT))
    return failures


def explain_coverage_failure() -> None:
    LOGGER.error("")
    LOGGER.error("dependabot.yml's own WARNING says a new excluded workspace needs a")
    LOGGER.error("row and that nothing fails when one is missing. This is that check.")


def explain_liveness_failure() -> None:
    LOGGER.error("")
    LOGGER.error("A dependabot ecosystem that produces nothing is indistinguishable")
    LOGGER.error("from one with nothing to do. That is why this check exists: cargo")
    LOGGER.error("went quiet for 113 days across 135 direct dependencies and nothing")
    LOGGER.error("noticed. Check Insights -> Dependency graph -> Dependabot for the")
    LOGGER.error("update-run log, which names the failure directly.")


def main(argv: list[str] | None = None) -> int:
    # WHY the split: COVERAGE is a property of the repository's own content, so it
    # belongs at PR time where the person who introduced the gap can fix it. LIVENESS
    # is a property of a service outside this repo, and running it on pull requests
    # would redden every open PR for something none of their authors did -- which is
    # how a check gets routed around.
    coverage_only = "--coverage-only" in (argv if argv is not None else sys.argv[1:])

    entries = declared_ecosystems(CONFIG)
    failures = evaluate(
        entries,
        [] if coverage_only else dependabot_prs(),
        dt.datetime.now(dt.timezone.utc),
        None if coverage_only else config_touched_at(),
        coverage_only,
    )

    if not failures:
        return 0
    if coverage_only:
        explain_coverage_failure()
    else:
        explain_liveness_failure()
    return 1


if __name__ == "__main__":
    logging.basicConfig(format="%(message)s", level=logging.INFO, stream=sys.stderr)
    raise SystemExit(main())
