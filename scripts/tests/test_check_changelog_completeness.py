from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT_PATH = Path(__file__).resolve().parents[1] / "check-changelog-completeness.py"
SPEC = importlib.util.spec_from_file_location("check_changelog_completeness", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {SCRIPT_PATH}")
ccc = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = ccc
SPEC.loader.exec_module(ccc)

DEFAULT_SECTIONS = [
    {"type": "feat", "section": "Features"},
    {"type": "fix", "section": "Bug Fixes"},
    {"type": "perf", "section": "Performance"},
    {"type": "refactor", "section": "Refactoring", "hidden": True},
    {"type": "docs", "section": "Documentation"},
    {"type": "test", "section": "Testing", "hidden": True},
    {"type": "chore", "section": "Maintenance", "hidden": True},
    {"type": "ci", "hidden": True},
    {"type": "style", "hidden": True},
]


def _run(cwd: Path, *args: str) -> None:
    subprocess.run(["git", *args], cwd=cwd, check=True, capture_output=True, text=True)


def _repo(tmp: str) -> Path:
    root = Path(tmp)
    _run(root, "init", "-q")
    _run(root, "config", "user.email", "test@example.com")
    _run(root, "config", "user.name", "Test")
    return root


def _commit(root: Path, path: str, content: str, subject: str) -> str:
    target = root / path
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(content, encoding="utf-8")
    _run(root, "add", "-A")
    _run(root, "commit", "-q", "-m", subject)
    result = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=root, check=True, capture_output=True, text=True
    )
    return result.stdout.strip()


def _config_file(root: Path, sections: list[dict[str, object]] = DEFAULT_SECTIONS) -> Path:
    path = root / "release-please-config.json"
    path.write_text(json.dumps({"changelog-sections": sections}), encoding="utf-8")
    return path


class RangeCommitParsing(unittest.TestCase):
    def test_type_and_pr_number_are_read_off_the_subject(self) -> None:
        commit = ccc.RangeCommit(
            sha="f7e4d3100fd1e09529f69d3f42aa524c6fdb5c88",
            subject="fix(agora): keep active-subscription gauge honest through into_receiver (#7105)",
        )
        self.assertEqual(commit.commit_type, "fix")
        self.assertEqual(commit.pr_number, "7105")
        self.assertEqual(commit.short_sha, "f7e4d31")

    def test_a_hidden_release_commit_still_parses_its_type(self) -> None:
        """WHY: hidden means 'do not require an entry', not 'cannot classify' -- the
        release-please bump commit itself must not be mistaken for an unparseable one."""
        commit = ccc.RangeCommit(sha="a" * 40, subject="chore(main): release 0.44.0 (#7007)")
        self.assertEqual(commit.commit_type, "chore")

    def test_a_non_conventional_subject_has_no_type(self) -> None:
        commit = ccc.RangeCommit(sha="a" * 40, subject="Merge branch 'main' into feature")
        self.assertIsNone(commit.commit_type)

    def test_a_bang_breaking_marker_does_not_hide_the_type(self) -> None:
        commit = ccc.RangeCommit(sha="a" * 40, subject="feat(organon)!: drop the legacy tool schema (#1)")
        self.assertEqual(commit.commit_type, "feat")


class VisibleCommitTypes(unittest.TestCase):
    def test_hidden_types_are_excluded(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = _config_file(root)
            self.assertEqual(
                ccc.visible_commit_types(root, path), {"feat", "fix", "perf", "docs"}
            )

    def test_a_config_with_no_visible_type_is_an_error(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = _config_file(root, [{"type": "chore", "hidden": True}])
            with self.assertRaises(ccc.ChangelogCompletenessError):
                ccc.visible_commit_types(root, path)

    def test_a_missing_config_is_an_error_not_an_empty_allowlist(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            with self.assertRaises(ccc.ChangelogCompletenessError):
                ccc.visible_commit_types(root, root / "nope.json")

    def test_a_config_outside_repo_root_is_rejected(self) -> None:
        """WHY(#7107): --config must not become a path-traversal escape hatch --
        confining it to repo_root is the fix for the SonarCloud finding on this line."""
        with tempfile.TemporaryDirectory() as tmp, tempfile.TemporaryDirectory() as other:
            root = Path(tmp)
            outside = _config_file(Path(other))
            with self.assertRaises(ccc.ChangelogCompletenessError):
                ccc.visible_commit_types(root, outside)


class MissingEntries(unittest.TestCase):
    """Driven over in-memory commits and changelog text -- the parsing this check
    exists to replace happens inside release-please, not here."""

    def test_a_commit_named_only_by_pr_number_counts_as_present(self) -> None:
        commits = [ccc.RangeCommit(sha="a" * 40, subject="fix(agora): thing (#7105)")]
        section = "### Bug Fixes\n\n* **agora:** thing ([#7105](...)) ([aaaaaaa](...))"
        self.assertEqual(ccc.missing_entries(commits, {"fix"}, section), [])

    def test_a_commit_named_only_by_sha_counts_as_present(self) -> None:
        sha = "f7e4d3100fd1e09529f69d3f42aa524c6fdb5c88"
        commits = [ccc.RangeCommit(sha=sha, subject="fix(agora): thing (#7105)")]
        section = "### Bug Fixes\n\n* **agora:** thing (f7e4d31)"
        self.assertEqual(ccc.missing_entries(commits, {"fix"}, section), [])

    def test_the_actual_defect_a_parsed_but_unlisted_commit_is_flagged(self) -> None:
        """This is exactly aletheia#7105/#7107: release-please parsed the commit
        (it is well-formed) but the entry never reached the section text."""
        commits = [
            ccc.RangeCommit(
                sha="f7e4d3100fd1e09529f69d3f42aa524c6fdb5c88",
                subject="fix(agora): keep active-subscription gauge honest (#7105)",
            ),
            ccc.RangeCommit(
                sha="81b62d0ca081d94f1872d3813cce0275b55cbeb8",
                subject="fix(agora): centralize channel-identity redaction (#7093)",
            ),
        ]
        section = "### Bug Fixes\n\n* **agora:** centralize channel-identity redaction ([#7093](...)) ([81b62d0](...))"
        missing = ccc.missing_entries(commits, {"fix"}, section)
        self.assertEqual([c.pr_number for c in missing], ["7105"])

    def test_a_hidden_type_commit_never_needs_an_entry(self) -> None:
        commits = [ccc.RangeCommit(sha="a" * 40, subject="chore(main): release 0.44.0 (#7007)")]
        self.assertEqual(ccc.missing_entries(commits, {"feat", "fix"}, ""), [])


class EndToEndOverARealRepo(unittest.TestCase):
    """Exercises commits_in_range and read_changelog_section against actual git
    plumbing -- the two functions this check trusts git, not a mock, to answer."""

    def test_passes_when_every_visible_commit_is_named(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = _repo(tmp)
            config = _config_file(root)
            _commit(root, "src/lib.rs", "v0", "chore: seed")
            _run(root, "tag", "v0.43.0")
            fix_sha = _commit(root, "src/agora.rs", "v1", "fix(agora): thing (#7093)")
            changelog = (
                "# Changelog\n\n"
                "## [0.44.0](https://example/compare/v0.43.0...v0.44.0) (2026-08-29)\n\n"
                "### Bug Fixes\n\n"
                f"* **agora:** thing ([#7093](...)) ([{fix_sha[:7]}](...))\n"
            )
            head = _commit(root, "CHANGELOG.md", changelog, "chore(main): release 0.44.0 (#7007)")
            missing = ccc.check(root, "v0.43.0", head, config, "CHANGELOG.md")
            self.assertEqual(missing, [])

    def test_fails_when_a_visible_commit_is_dropped(self) -> None:
        """The exact scenario in aletheia#7107: two fix(agora) commits land, the
        generated changelog names only one, and the run must not report clean."""
        with tempfile.TemporaryDirectory() as tmp:
            root = _repo(tmp)
            config = _config_file(root)
            _commit(root, "src/lib.rs", "v0", "chore: seed")
            _run(root, "tag", "v0.43.0")
            _commit(root, "src/agora.rs", "v1", "fix(agora): centralize redaction (#7093)")
            dropped_sha = _commit(
                root, "src/agora2.rs", "v2", "fix(agora): keep gauge honest (#7105)"
            )
            changelog = (
                "# Changelog\n\n"
                "## [0.44.0](https://example/compare/v0.43.0...v0.44.0) (2026-08-29)\n\n"
                "### Bug Fixes\n\n"
                "* **agora:** centralize redaction ([#7093](...)) ([81b62d0](...))\n"
            )
            head = _commit(root, "CHANGELOG.md", changelog, "chore(main): release 0.44.0 (#7007)")
            missing = ccc.check(root, "v0.43.0", head, config, "CHANGELOG.md")
            self.assertEqual([c.pr_number for c in missing], ["7105"])
            self.assertEqual(missing[0].short_sha, dropped_sha[:7])

    def test_only_the_newest_section_is_checked_against(self) -> None:
        """An older release's entries must never satisfy a newer release's commits --
        that would let a real drop hide behind an unrelated past entry."""
        with tempfile.TemporaryDirectory() as tmp:
            root = _repo(tmp)
            config = _config_file(root)
            old_sha = _commit(root, "src/lib.rs", "v0", "fix(agora): old thing (#1)")
            changelog_v1 = (
                "# Changelog\n\n"
                "## [0.43.0](https://example/compare/v0.42.0...v0.43.0) (2026-08-20)\n\n"
                "### Bug Fixes\n\n"
                f"* **agora:** old thing ([#1](...)) ([{old_sha[:7]}](...))\n"
            )
            _commit(root, "CHANGELOG.md", changelog_v1, "chore(main): release 0.43.0 (#2)")
            _run(root, "tag", "v0.43.0")
            new_sha = _commit(root, "src/agora.rs", "v3", "fix(agora): new thing (#3)")
            changelog_v2 = (
                "## [0.44.0](https://example/compare/v0.43.0...v0.44.0) (2026-08-29)\n\n"
                "### Bug Fixes\n\n"
                "* **agora:** an unrelated entry naming neither the PR nor the SHA\n\n"
                + changelog_v1
            )
            head = _commit(root, "CHANGELOG.md", changelog_v2, "chore(main): release 0.44.0 (#4)")
            missing = ccc.check(root, "v0.43.0", head, config, "CHANGELOG.md")
            self.assertEqual([c.short_sha for c in missing], [new_sha[:7]])


class RefValidation(unittest.TestCase):
    """WHY(#7107): --from-ref/--to-ref reach git as bare argv. A value shaped like a
    git option (leading "-") must never be treated as a revision."""

    def test_a_leading_dash_ref_is_rejected(self) -> None:
        with self.assertRaises(ccc.ChangelogCompletenessError):
            ccc.validate_ref("--upload-pack=x", "--from-ref")

    def test_an_ordinary_tag_or_sha_is_accepted(self) -> None:
        self.assertEqual(ccc.validate_ref("v0.43.0", "--from-ref"), "v0.43.0")
        self.assertEqual(
            ccc.validate_ref("f7e4d3100fd1e09529f69d3f42aa524c6fdb5c88", "--to-ref"),
            "f7e4d3100fd1e09529f69d3f42aa524c6fdb5c88",
        )

    def test_commits_in_range_rejects_an_option_shaped_from_ref_before_calling_git(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = _repo(tmp)
            _commit(root, "src/lib.rs", "v0", "chore: seed")
            with self.assertRaises(ccc.ChangelogCompletenessError):
                ccc.commits_in_range(root, "--output=/tmp/pwned", "HEAD")


class BackfilledEntryStaysCovered(unittest.TestCase):
    """Regression guard for aletheia#7270: release-please dropped commit 5e62b60
    (`fix(poiesis) #7062`) from the shipped 0.44.0 changelog -- the same silent
    parser drop #7107/#7273 describe, on a second commit #7273 named but did not
    backfill. This reads the real, checked-in CHANGELOG.md (not a synthetic repo)
    so it fails on the pre-#7270 tree and passes once the entry is restored."""

    def test_commit_5e62b60_is_named_in_the_real_0_44_0_section(self) -> None:
        repo_root = Path(__file__).resolve().parents[2]
        lines = (repo_root / "CHANGELOG.md").read_text(encoding="utf-8").splitlines()
        heading_indices = [
            i for i, line in enumerate(lines) if ccc.CHANGELOG_HEADING_RE.match(line)
        ]
        start = next(i for i in heading_indices if lines[i].startswith("## [0.44.0]"))
        end = next((i for i in heading_indices if i > start), len(lines))
        section = "\n".join(lines[start:end])

        dropped_commit = ccc.RangeCommit(
            sha="5e62b60d075f991b6990743a7d2792c61ea94c07",
            subject=(
                "fix(poiesis): consolidate small forks in ids, XML, charts, sources, "
                "and model helpers (#7062)"
            ),
        )
        self.assertEqual(ccc.missing_entries([dropped_commit], {"fix"}, section), [])


if __name__ == "__main__":
    unittest.main()
