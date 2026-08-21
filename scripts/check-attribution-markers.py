#!/usr/bin/env python3
"""Scan a PR's title, body and commits for generated-output attribution markers.

WHY(#6874): the previous shell version could not distinguish "I looked and found a
violation" from "I could not look." It fetched the PR title and body through `gh`
(GraphQL) and exited 1 either way, so during a GitHub GraphQL degradation it went red on
every open PR with a 503 and no scan output at all. A maintainer seeing red reasonably
starts hunting a marker that does not exist; that happened three times in one day across
four PRs. Meanwhile `hybrid-gate / ai-attribution`, which scans commits and needs no API
call, passed on the same commits -- the two disagreed purely on transport availability.

Failing closed is the right direction. The defect was that the failure was
indistinguishable from a finding, and a check that goes red for reasons nobody can act
on gets re-run reflexively, then called flaky, and a check called flaky is one nobody
reads -- which is how a real marker eventually gets through.

Two corrections, in that order of importance:

  * **The subject comes from `GITHUB_EVENT_PATH`, not the API.** The payload is already
    on disk, written by the same server that would have answered the query. That removes
    the dependency rather than hardening it: there is no fetch to retry, no backoff to
    tune, and no outage that can turn a clean PR red.

  * **An unreadable subject exits 2, a marker exits 1**, with different messages. Both
    are non-zero -- a check that cannot read its subject must never report clean -- but
    the red now says which of the two happened, to a human and to a machine.
"""

from __future__ import annotations

import json
import logging
import os
import subprocess
import sys
from pathlib import Path

LOGGER = logging.getLogger("check-attribution-markers")

REPO_ROOT = Path(__file__).resolve().parents[1]
PATTERN_FILE = REPO_ROOT / ".github" / "no-ai-attribution-patterns.txt"

EXIT_CLEAN = 0
EXIT_MARKER_FOUND = 1
EXIT_SUBJECT_UNREADABLE = 2


class UnreadableSubject(Exception):
    """The thing to be scanned could not be obtained. Never confuse this with clean."""


def assert_pattern_list(path: Path = PATTERN_FILE) -> Path:
    """Confirm the checked-in pattern list exists and declares something.

    WHY explicit: `grep -f` on an EMPTY pattern file matches nothing and exits 1, which
    is indistinguishable from a clean subject. A pattern list that has been emptied
    would silently pass every PR -- a green meaning nothing was looked at, which is the
    same defect as the red meaning nothing was looked at.
    """
    try:
        text = path.read_text(encoding="utf-8")
    except (OSError, UnicodeError) as error:
        raise UnreadableSubject(f"cannot read the pattern list {path.name}: {error}") from error
    if not [line for line in text.splitlines() if line.strip() and not line.startswith("#")]:
        raise UnreadableSubject(
            f"{path.name} declares no patterns; this check would pass anything"
        )
    return path


def matches(text: str, pattern_file: Path) -> list[str]:
    """Lines of `text` carrying a marker, as `grep -n` renders them.

    WHY grep and not Python's `re`: this pattern list is POSIX ERE -- it uses
    `[[:space:]]` -- and it is consumed by the gate's own commit scan through `grep -E`.
    Python's `re` does not implement POSIX bracket expressions and parses `[[:space:]]`
    as a character set of the letters in "space", so a Python matcher reads the file
    without error and silently fails to match the very marker the first pattern names.
    Re-implementing the dialect here would be a second engine to keep in step; asking
    the same engine keeps the file the one authority on what a marker is.

    grep's own exit codes carry the distinction this whole check is about: 0 found,
    1 looked and found nothing, 2 could not evaluate.
    """
    result = subprocess.run(
        ["grep", "-niE", "-f", str(pattern_file)],
        input=text, capture_output=True, text=True, check=False,
    )
    if result.returncode == 0:
        return result.stdout.splitlines()
    if result.returncode == 1:
        return []
    raise UnreadableSubject(
        f"grep could not evaluate {pattern_file.name}: {result.stderr.strip()}"
    )


def pr_subject(event_path: str | None) -> tuple[str, str]:
    """Return (title, body) from the workflow's own event payload.

    WHY not `gh pr view`: that is a GraphQL call, and its failure was indistinguishable
    from a finding. The payload is on disk before the job starts.
    """
    if not event_path:
        raise UnreadableSubject("GITHUB_EVENT_PATH is not set; there is no PR to scan")
    try:
        payload = json.loads(Path(event_path).read_text(encoding="utf-8"))
    except (OSError, UnicodeError, ValueError) as error:
        raise UnreadableSubject(f"cannot read the event payload: {error}") from error
    pull_request = payload.get("pull_request")
    if not isinstance(pull_request, dict):
        raise UnreadableSubject("the event payload carries no pull_request")
    # WHY `or ""` and not a missing-key error: GitHub sends `"body": null` for a PR with
    # an empty description, which is ordinary and must scan clean rather than blow up.
    return pull_request.get("title") or "", pull_request.get("body") or ""


def commit_messages(base_ref: str) -> dict[str, str]:
    """Every commit message on this branch since `base_ref`."""
    listing = subprocess.run(
        ["git", "log", "--format=%H", f"origin/{base_ref}..HEAD"],
        capture_output=True, text=True, check=False, cwd=REPO_ROOT,
    )
    if listing.returncode != 0:
        raise UnreadableSubject(f"cannot list commits since {base_ref}: {listing.stderr.strip()}")
    messages = {}
    for sha in listing.stdout.split():
        one = subprocess.run(
            ["git", "log", "-1", "--format=%B", sha],
            capture_output=True, text=True, check=False, cwd=REPO_ROOT,
        )
        if one.returncode != 0:
            raise UnreadableSubject(f"cannot read commit {sha}: {one.stderr.strip()}")
        messages[sha] = one.stdout
    return messages


def scan(pattern_file: Path, subjects: dict[str, str]) -> bool:
    """Report every marker across `subjects`; return True when any was found."""
    dirty = False
    for where, text in subjects.items():
        for line in matches(text, pattern_file):
            dirty = True
            LOGGER.error("::error::%s: %s", where, line.strip())
    return dirty


def main(argv: list[str] | None = None) -> int:
    args = argv if argv is not None else sys.argv[1:]
    base_ref = os.environ.get("BASE_REF", "main")

    try:
        pattern_file = assert_pattern_list()
        subjects = dict(zip(("PR title", "PR body"), pr_subject(os.environ.get("GITHUB_EVENT_PATH"))))
        if "--commits" in args:
            subjects.update(
                {f"commit {sha}": message for sha, message in commit_messages(base_ref).items()}
            )
        # WHY inside the try: `matches` raises when grep cannot evaluate the pattern
        # list, and that is an I-could-not-look, not a finding. Scanning outside would
        # let it escape as a traceback -- a third rendering of the same red.
        dirty = scan(pattern_file, subjects)
    except UnreadableSubject as error:
        # The whole point of this exit code. A red here means the check could not LOOK;
        # it does not mean a marker was found, and it must not be read as clean either.
        LOGGER.error("::error::check-attribution-markers: could not read what it scans")
        LOGGER.error("::error::  %s", error)
        LOGGER.error("::error::  No scan was performed. This is NOT a finding, and it is")
        LOGGER.error("::error::  NOT a pass. Re-run once the cause above is addressed.")
        return EXIT_SUBJECT_UNREADABLE

    if dirty:
        LOGGER.error("::error::check-attribution-markers: attribution marker(s) found above.")
        LOGGER.error("::error::  Remove them and edit the PR, or reword the commit and push.")
        return EXIT_MARKER_FOUND

    LOGGER.info(
        "check-attribution-markers: %d subject(s) clean of attribution markers",
        len(subjects),
    )
    return EXIT_CLEAN


if __name__ == "__main__":
    logging.basicConfig(format="%(message)s", level=logging.INFO, stream=sys.stderr)
    raise SystemExit(main())
