from __future__ import annotations

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

SCRIPT_PATH = Path(__file__).resolve().parents[1] / "check-attribution-markers.py"
SPEC = importlib.util.spec_from_file_location("check_attribution_markers", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {SCRIPT_PATH}")
am = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = am
SPEC.loader.exec_module(am)

# WHY assembled from parts rather than written out: a literal attribution marker checked
# into this repository is the very thing every scanner here exists to reject, and a test
# fixture is not exempt from a tree scan. Joining at runtime keeps the string out of the
# file while giving the assertions the real shape to match.
MARKER = "Co-Authored-By: " + "Claude" + " <noreply@example.invalid>"
ROBOT = "\U0001f916"


def event_file(tmp: str, title: str, body: str | None) -> str:
    path = Path(tmp) / "event.json"
    path.write_text(
        json.dumps({"pull_request": {"title": title, "body": body}}), encoding="utf-8"
    )
    return str(path)


class Patterns(unittest.TestCase):
    def test_patterns_come_from_the_checked_in_file(self) -> None:
        """WHY not a Python copy: this list is consumed by this check AND the gate's
        commit scan. A second copy diverges the first time a marker is added, and it
        diverges in the permissive direction."""
        self.assertTrue(
            am.matches(MARKER, am.assert_pattern_list()),
            "the live pattern list must match the real marker",
        )

    def test_an_empty_pattern_file_is_an_error_not_a_pass(self) -> None:
        """WHY loud: zero patterns makes every scan report clean, which is the exact
        failure this whole check is about -- a green that means nothing was looked at."""
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "patterns.txt"
            path.write_text("# only a comment\n", encoding="utf-8")
            with self.assertRaises(am.UnreadableSubject):
                am.assert_pattern_list(path)

    def test_a_missing_pattern_file_is_unreadable_not_clean(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaises(am.UnreadableSubject):
                am.assert_pattern_list(Path(tmp) / "absent.txt")

    def test_matching_is_case_insensitive(self) -> None:
        self.assertTrue(am.matches(MARKER.upper(), am.assert_pattern_list()))

    def test_a_posix_bracket_expression_in_the_live_list_actually_matches(self) -> None:
        """WHY pinned: the first pattern uses `[[:space:]]`, POSIX ERE. Python's `re`
        parses that as a set of the letters in "space" and reads the file without any
        error, so a Python matcher here would silently miss the very marker that pattern
        names. This fails if the matcher is ever swapped for one that cannot read the
        file's dialect."""
        self.assertTrue(
            am.matches("Co-Authored-By:   Someone via " + "Anthropic", am.assert_pattern_list())
        )


class SubjectFromTheEventPayload(unittest.TestCase):
    """The correction: the subject is on disk, so no outage can turn a clean PR red."""

    def test_title_and_body_are_read_from_the_payload(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = event_file(tmp, "fix: something", "a body")
            self.assertEqual(am.pr_subject(path), ("fix: something", "a body"))

    def test_a_null_body_scans_as_empty_rather_than_failing(self) -> None:
        """WHY: GitHub sends a null body for a PR with no description. That is
        ordinary, and treating it as unreadable would red every such PR."""
        with tempfile.TemporaryDirectory() as tmp:
            self.assertEqual(am.pr_subject(event_file(tmp, "fix: x", None)), ("fix: x", ""))

    def test_an_unset_event_path_is_unreadable(self) -> None:
        with self.assertRaises(am.UnreadableSubject):
            am.pr_subject(None)

    def test_a_malformed_payload_is_unreadable(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "event.json"
            path.write_text("{not json", encoding="utf-8")
            with self.assertRaises(am.UnreadableSubject):
                am.pr_subject(str(path))

    def test_a_payload_without_a_pull_request_is_unreadable(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "event.json"
            path.write_text(json.dumps({"push": {}}), encoding="utf-8")
            with self.assertRaises(am.UnreadableSubject):
                am.pr_subject(str(path))


class ExitCodesAreDistinct(unittest.TestCase):
    """Done-when clause 1: an unreadable subject and a found marker must not render as
    the same red. They were the same exit code and the same absence of output, which is
    how a 503 sent maintainers hunting a marker that did not exist."""

    def _main(self, title: str, body: str | None) -> int:
        with tempfile.TemporaryDirectory() as tmp:
            with mock.patch.dict(
                "os.environ", {"GITHUB_EVENT_PATH": event_file(tmp, title, body)}
            ):
                return am.main([])

    def test_a_clean_pr_passes(self) -> None:
        self.assertEqual(self._main("fix: a real change", "no markers here"), am.EXIT_CLEAN)

    def test_a_marker_in_the_body_fails_with_the_marker_code(self) -> None:
        """Done-when clause 3: red on that shape, green without it, proven by the pair
        of this test and the one above."""
        self.assertEqual(self._main("fix: x", "work\n\n" + MARKER + "\n"), am.EXIT_MARKER_FOUND)

    def test_a_marker_in_the_title_fails_too(self) -> None:
        self.assertEqual(self._main("fix: " + ROBOT + " x", "clean"), am.EXIT_MARKER_FOUND)

    def test_an_unreadable_subject_fails_with_a_DIFFERENT_code(self) -> None:
        with mock.patch.dict("os.environ", {"GITHUB_EVENT_PATH": ""}):
            code = am.main([])
        self.assertEqual(code, am.EXIT_SUBJECT_UNREADABLE)
        self.assertNotEqual(code, am.EXIT_MARKER_FOUND)
        self.assertNotEqual(code, am.EXIT_CLEAN)

    def test_the_unreadable_message_names_the_cause_and_denies_both_readings(self) -> None:
        """WHY assert the wording: the exit code separates the two for a machine, but a
        maintainer reads the log. It has to say NOT a finding and NOT a pass, because
        the previous behaviour taught people to read the red as the first."""
        with mock.patch.dict("os.environ", {"GITHUB_EVENT_PATH": ""}), self.assertLogs(
            am.LOGGER, level="ERROR"
        ) as captured:
            am.main([])
        text = "\n".join(captured.output)
        self.assertIn("could not read what it scans", text)
        self.assertIn("NOT a finding", text)
        self.assertIn("NOT a pass", text)

    def test_the_marker_message_does_not_claim_it_could_not_look(self) -> None:
        """The other direction of the same confusion."""
        with tempfile.TemporaryDirectory() as tmp, mock.patch.dict(
            "os.environ", {"GITHUB_EVENT_PATH": event_file(tmp, "fix: x", MARKER)}
        ), self.assertLogs(am.LOGGER, level="ERROR") as captured:
            am.main([])
        text = "\n".join(captured.output)
        self.assertIn("attribution marker(s) found", text)
        self.assertNotIn("could not read", text)


class NoNetworkCall(unittest.TestCase):
    def test_the_scan_makes_no_subprocess_call_without_commits(self) -> None:
        """Done-when clause 2, in the form option 3 leaves it: there is no fetch to fail
        transiently, so a transient failure cannot fail the check. Asserted by the
        absence of any `gh` invocation on the default path -- a retry loop that is never
        reached is not the same as a dependency that is not there."""
        real = am.subprocess.run

        def no_gh(argv, *a, **kw):
            if argv and argv[0] == "gh":
                raise AssertionError(f"the scan must not call gh: {argv}")
            return real(argv, *a, **kw)

        with tempfile.TemporaryDirectory() as tmp, mock.patch.dict(
            "os.environ", {"GITHUB_EVENT_PATH": event_file(tmp, "fix: x", "clean")}
        ), mock.patch.object(am.subprocess, "run", side_effect=no_gh):
            self.assertEqual(am.main([]), am.EXIT_CLEAN)


class CommitScan(unittest.TestCase):
    def test_a_marker_in_a_commit_message_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp, mock.patch.dict(
            "os.environ", {"GITHUB_EVENT_PATH": event_file(tmp, "fix: x", "clean")}
        ), mock.patch.object(
            am, "commit_messages", return_value={"abc123": "work\n\n" + MARKER + "\n"}
        ):
            self.assertEqual(am.main(["--commits"]), am.EXIT_MARKER_FOUND)

    def test_an_unlistable_history_is_unreadable_not_clean(self) -> None:
        with tempfile.TemporaryDirectory() as tmp, mock.patch.dict(
            "os.environ", {"GITHUB_EVENT_PATH": event_file(tmp, "fix: x", "clean")}
        ), mock.patch.object(am, "commit_messages", side_effect=am.UnreadableSubject("no git")):
            self.assertEqual(am.main(["--commits"]), am.EXIT_SUBJECT_UNREADABLE)


if __name__ == "__main__":
    unittest.main()
