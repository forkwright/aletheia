#!/usr/bin/env python3
"""Verify the generated changelog's entries cover every visible commit in a release range.

WHY(#7107): release-please's commit parser silently drops a commit it cannot parse
instead of failing the run. 0.44.0 (#7007) omitted its most operationally significant
fix, `fix(agora): keep active-subscription gauge honest through into_receiver` (#7105),
because the commit's `` ```rust `` body fence held an unbalanced `(`. The parser logged
`commit could not be parsed` at debug level and the action still reported success --
release-please.yml's own `steps.release.outputs.release_created` was `true`, the draft
release existed, and nothing on the PR, the run conclusion, or the changelog said a
commit was skipped. The only trace was a job-log line nobody reads on a green run.

This mirrors the "Assert every declared extra-file actually moved" step already in
release-please.yml (aletheia#6713) for the identical shape of defect: release-please
does not fail, or even exit non-zero, when it silently drops something it was supposed
to carry forward. That guard diffs a declared file list against what actually changed;
this one diffs the conventional-commit set in the release range against what the
generated changelog actually names.

A commit "has an entry" if the changelog text for this release names either its PR
number (`#NNNN`, appearing inside release-please's `[#NNNN](url)` link -- the form
every squash-merged commit carries) or its abbreviated SHA (the form release-please
emits regardless, inside its own `[abcdef1](url)` link). Requiring only one of the
two, not both, keeps this from being a second copy of release-please's own template.
"""

from __future__ import annotations

import argparse
import json
import logging
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

LOGGER = logging.getLogger("check-changelog-completeness")

DEFAULT_CONFIG_PATH = "release-please-config.json"
DEFAULT_CHANGELOG_PATH = "CHANGELOG.md"

# WHY anchored, not `re.match`: a scope may itself contain `):`-shaped text in theory,
# and anchoring the whole header keeps a stray colon later in the subject from being
# mistaken for the type/scope separator.
CONVENTIONAL_HEADER_RE = re.compile(
    r"^(?P<type>[a-z]+)(?:\([^)]*\))?!?:\s+\S"
)
PR_NUMBER_RE = re.compile(r"\(#(\d+)\)\s*$")
CHANGELOG_HEADING_RE = re.compile(r"^## \[")


class ChangelogCompletenessError(RuntimeError):
    """Raised when the release range or changelog cannot be read at all."""


@dataclass(frozen=True)
class RangeCommit:
    sha: str
    subject: str

    @property
    def short_sha(self) -> str:
        return self.sha[:7]

    @property
    def commit_type(self) -> str | None:
        match = CONVENTIONAL_HEADER_RE.match(self.subject)
        return match.group("type") if match else None

    @property
    def pr_number(self) -> str | None:
        match = PR_NUMBER_RE.search(self.subject)
        return match.group(1) if match else None


def run_git(repo_root: Path, *args: str) -> str:
    result = subprocess.run(
        ["git", "-C", str(repo_root), *args],
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        raise ChangelogCompletenessError(
            f"git {' '.join(args)} failed: {result.stderr.strip()}"
        )
    return result.stdout


def visible_commit_types(config_path: Path) -> set[str]:
    """Types release-please's own config renders into a visible changelog section.

    WHY read from `release-please-config.json` rather than hardcoded: this list is
    exactly the one release-please itself uses (`changelog-sections`), so a future edit
    there -- adding a section, hiding one -- keeps this check in step without a second
    edit anyone has to remember to make.
    """
    try:
        raw = json.loads(config_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ChangelogCompletenessError(
            f"cannot read release-please config at {config_path}: {exc}"
        ) from exc

    sections = raw.get("changelog-sections")
    if not isinstance(sections, list) or not sections:
        raise ChangelogCompletenessError(
            f"{config_path} has no non-empty 'changelog-sections'"
        )

    visible = {
        entry["type"]
        for entry in sections
        if isinstance(entry, dict) and entry.get("type") and not entry.get("hidden")
    }
    if not visible:
        raise ChangelogCompletenessError(
            f"{config_path}'s 'changelog-sections' names no visible type"
        )
    return visible


def commits_in_range(repo_root: Path, from_ref: str, to_ref: str) -> list[RangeCommit]:
    # WHY %x1f (unit separator) rather than a printable delimiter: a commit subject can
    # itself carry punctuation of any kind, but never a control character, so this is
    # the one split point guaranteed not to collide with real content.
    raw = run_git(
        repo_root, "log", "--no-merges", f"--format=%H%x1f%s", f"{from_ref}..{to_ref}"
    )
    commits: list[RangeCommit] = []
    for line in raw.splitlines():
        if not line:
            continue
        sha, _, subject = line.partition("\x1f")
        commits.append(RangeCommit(sha=sha, subject=subject))
    return commits


def read_changelog_section(repo_root: Path, ref: str, changelog_path: str) -> str:
    """The newest version's section of the changelog as of `ref`.

    release-please always prepends the new release at the top of the file, directly
    under the `# Changelog` title, so the newest section is everything before the
    SECOND `## [` heading (the first heading belongs to the release under test).
    """
    full_text = run_git(repo_root, "show", f"{ref}:{changelog_path}")
    lines = full_text.splitlines()
    heading_indices = [i for i, line in enumerate(lines) if CHANGELOG_HEADING_RE.match(line)]
    if not heading_indices:
        raise ChangelogCompletenessError(
            f"{changelog_path} at {ref} has no '## [' release heading"
        )
    end = heading_indices[1] if len(heading_indices) > 1 else len(lines)
    return "\n".join(lines[heading_indices[0] : end])


def missing_entries(
    commits: list[RangeCommit], visible_types: set[str], section_text: str
) -> list[RangeCommit]:
    missing = []
    for commit in commits:
        if commit.commit_type not in visible_types:
            continue
        # WHY a trailing-digit lookahead: release-please writes the PR number as link
        # text, "[#7093](url)", not "(#7093)" -- a bare substring check on "#7093"
        # would otherwise also accept "#70931", a different PR that merely starts with
        # the same digits.
        named_by_pr = commit.pr_number is not None and re.search(
            rf"#{re.escape(commit.pr_number)}(?!\d)", section_text
        )
        named_by_sha = commit.short_sha in section_text
        if not named_by_pr and not named_by_sha:
            missing.append(commit)
    return missing


def check(
    repo_root: Path,
    from_ref: str,
    to_ref: str,
    config_path: Path,
    changelog_path: str,
) -> list[RangeCommit]:
    visible_types = visible_commit_types(config_path)
    commits = commits_in_range(repo_root, from_ref, to_ref)
    section_text = read_changelog_section(repo_root, to_ref, changelog_path)
    return missing_entries(commits, visible_types, section_text)


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Fail when a feat/fix/perf/docs commit in a release range produced no "
            "changelog entry (aletheia#7107)."
        )
    )
    parser.add_argument(
        "--repo-root",
        type=Path,
        default=Path(__file__).resolve().parents[1],
        help="repository root to run git in",
    )
    parser.add_argument(
        "--from-ref",
        required=True,
        help="exclusive start of the release range, e.g. the previous release tag",
    )
    parser.add_argument(
        "--to-ref",
        required=True,
        help="inclusive end of the release range, e.g. the release branch head",
    )
    parser.add_argument(
        "--config",
        type=Path,
        default=None,
        help=f"path to release-please-config.json (default: <repo-root>/{DEFAULT_CONFIG_PATH})",
    )
    parser.add_argument(
        "--changelog-path",
        default=DEFAULT_CHANGELOG_PATH,
        help="changelog path as tracked by git, relative to repo-root",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    logging.basicConfig(level=logging.INFO, format="%(message)s")
    args = parse_args(sys.argv[1:] if argv is None else argv)
    repo_root = args.repo_root.resolve()
    config_path = args.config or (repo_root / DEFAULT_CONFIG_PATH)

    try:
        missing = check(
            repo_root, args.from_ref, args.to_ref, config_path, args.changelog_path
        )
    except ChangelogCompletenessError as exc:
        LOGGER.error("changelog completeness check could not run: %s", exc)
        return 2

    if missing:
        LOGGER.error(
            "changelog is missing %d entr%s for commits in %s..%s "
            "(aletheia#7107 -- release-please dropped one of these without "
            "failing the run):",
            len(missing),
            "y" if len(missing) == 1 else "ies",
            args.from_ref,
            args.to_ref,
        )
        for commit in missing:
            LOGGER.error("  - %s %s", commit.short_sha, commit.subject)
        return 1

    LOGGER.info(
        "changelog covers every visible commit in %s..%s", args.from_ref, args.to_ref
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
