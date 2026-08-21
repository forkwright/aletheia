#!/usr/bin/env python3
"""A PR title that release-please cannot parse contributes nothing to the release.

WHY(#6806): merges here are squashes, so the PR title becomes the commit subject, and
that subject is release-please's only input. A prose title -- "Query budgets and
cancellation reasons, ..." -- parses as no type at all: the PR's content is invisible
to both the changelog and the version bump. Five dense PRs merged in one day with prose
titles, which is why the release PR open at the time still proposed a patch bump.

Nothing surfaces this. The merge succeeds, the changelog is simply shorter than the
work, and the version is lower than it should be. It is only visible to someone
comparing a release against the PRs it contains, which nobody does.

The accepted types are read from `release-please-config.json`, never restated here.
That file's `changelog-sections` IS the definition of a type this project can place in
a changelog; a list in this script would be a second copy that goes stale the first
time a section is added, and it would go stale in the permissive direction -- accepting
a title release-please then drops.
"""

from __future__ import annotations

import json
import logging
import os
import re
import sys
from pathlib import Path

LOGGER = logging.getLogger("check-pr-title-conventional")

CONFIG = Path(__file__).resolve().parents[1] / "release-please-config.json"


def accepted_types(config_path: Path) -> list[str]:
    """The commit types release-please can place in a changelog, from its own config.

    A `hidden` section still counts: hidden means "do not print this section", not
    "cannot parse this type" -- a `chore:` title is parsed, bumps nothing, and is
    correctly invisible. Excluding hidden types here would reject the titles
    release-please itself generates.
    """
    config = json.loads(config_path.read_text(encoding="utf-8"))
    sections = config.get("changelog-sections", [])
    types = [str(section["type"]) for section in sections if "type" in section]
    if not types:
        raise SystemExit(
            f"{config_path.name} declares no changelog-sections types; "
            "this check cannot derive what it accepts"
        )
    return types


def violation(title: str, types: list[str]) -> str | None:
    """Return why `title` is unusable to release-please, or None when it is fine."""
    if not title.strip():
        return "the title is empty"

    pattern = re.compile(
        r"^(?P<type>[a-z]+)"
        r"(?:\((?P<scope>[^()\n]+)\))?"
        r"(?P<breaking>!)?"
        r": (?P<description>.+)$"
    )
    match = pattern.match(title)
    if match is None:
        # A capitalised or spaced type is the common near-miss, and the generic
        # message reads as though the whole shape were wrong. Say which half is.
        #
        # SECURITY: whitespace is collapsed FIRST and every optional here matches at
        # most one character. The obvious spelling -- `\s*` around two optional groups
        # -- is super-linear: on a title of N spaces and no colon, the engine tries
        # every way of splitting that run across three unbounded quantifiers. A PR
        # title is attacker-supplied, so that is a denial of service, not a slow regex.
        compact = " ".join(title.split())
        loose = re.match(r"^(?P<type>[A-Za-z]+) ?(?:\([^()\n]*\))? ?!? ?:", compact)
        if loose is not None:
            found = loose.group("type")
            if found.lower() in types:
                return (
                    f"the type must be lowercase and followed by `: ` exactly -- "
                    f"release-please matches `{found.lower()}`, not `{found}`"
                )
        return (
            "the title is not conventional-commit format "
            "(`type(scope): description`, or `type!: description` for a break)"
        )

    found = match.group("type")
    if found not in types:
        return (
            f"`{found}` is not a type this project's changelog can place; "
            f"release-please-config.json declares {', '.join(sorted(types))}"
        )

    if not match.group("description").strip():
        return "the description after the colon is empty"

    return None


def main() -> int:
    title = os.environ.get("PR_TITLE", "")
    problem = violation(title, accepted_types(CONFIG))
    if problem is None:
        LOGGER.info("check-pr-title-conventional: %s", title)
        return 0

    LOGGER.error("check-pr-title-conventional: %s", problem)
    LOGGER.error("  Title: %s", title)
    LOGGER.error("")
    LOGGER.error("This repository squash-merges, so the PR title becomes the commit")
    LOGGER.error("subject and is release-please's only input. A title it cannot parse")
    LOGGER.error("does not fail anything at merge -- the work simply never appears in")
    LOGGER.error("the changelog and never moves the version.")
    LOGGER.error("")
    LOGGER.error("Edit the PR title; no push is needed.")
    return 1


if __name__ == "__main__":
    logging.basicConfig(format="%(message)s", level=logging.INFO, stream=sys.stderr)
    raise SystemExit(main())
