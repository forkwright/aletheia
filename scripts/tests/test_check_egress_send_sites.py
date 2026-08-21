from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

SCRIPT_PATH = Path(__file__).resolve().parents[1] / "check-egress-send-sites.py"
SPEC = importlib.util.spec_from_file_location("check_egress_send_sites", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {SCRIPT_PATH}")
eg = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = eg
SPEC.loader.exec_module(eg)

SEND = 'pub async fn f(c: &reqwest::Client) { let _ = c.get("https://x").send().await; }\n'


class SendSiteDiscovery(unittest.TestCase):
    def test_finds_a_send_call_and_reports_its_line(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            src = root / "crates" / "organon" / "src"
            src.mkdir(parents=True)
            (src / "thing.rs").write_text("// header\n" + SEND, encoding="utf-8")
            with mock.patch.object(eg, "SCANNED_ROOTS", ("crates/organon/src",)):
                found = eg.send_sites(root)
            self.assertEqual(found, {"crates/organon/src/thing.rs": [2]})

    def test_a_send_call_named_in_prose_is_not_a_send_site(self) -> None:
        """WHY: the doc comment on a routed call site explains the shape it replaced.
        Naming `.send()` there made the fixed site report itself as unaccounted-for --
        documenting the rule tripped the rule."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            src = root / "crates" / "organon" / "src"
            src.mkdir(parents=True)
            (src / "documented.rs").write_text(
                "/// A call that went back to `client.send()` would skip the gate.\n"
                "// so would this one: c.send()\n"
                "pub fn f() {}\n",
                encoding="utf-8",
            )
            with mock.patch.object(eg, "SCANNED_ROOTS", ("crates/organon/src",)):
                found = eg.send_sites(root)
            self.assertEqual(found, {})

    def test_a_url_containing_a_double_slash_does_not_hide_its_send_call(self) -> None:
        """WHY: truncating at the first `//` instead of skipping whole-line comments
        would cut this line inside the URL literal and miss a real site -- the one
        direction this check must never fail in."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            src = root / "crates" / "organon" / "src"
            src.mkdir(parents=True)
            (src / "urly.rs").write_text(
                'pub async fn f(c: &reqwest::Client) { c.get("https://x").send().await; }\n',
                encoding="utf-8",
            )
            with mock.patch.object(eg, "SCANNED_ROOTS", ("crates/organon/src",)):
                found = eg.send_sites(root)
            self.assertEqual(found, {"crates/organon/src/urly.rs": [1]})

    def test_a_missing_scanned_root_is_an_error_not_an_empty_result(self) -> None:
        """WHY: an empty scan and a scan that found nothing look identical, and the
        first would silently pass this check forever after a directory move."""
        with tempfile.TemporaryDirectory() as tmp:
            with mock.patch.object(eg, "SCANNED_ROOTS", ("crates/gone/src",)):
                with self.assertRaises(SystemExit):
                    eg.send_sites(Path(tmp))


class Accounting(unittest.TestCase):
    """Every send site must be sanctioned, exempted, or tracked against an open issue.

    Driven over a synthetic tree so the cases are the shapes, not this repository's
    current six sites -- which would make the tests restate the allowlist.
    """

    def _tree(self, tmp: str, names: list[str]) -> Path:
        root = Path(tmp)
        src = root / "crates" / "organon" / "src"
        src.mkdir(parents=True)
        for name in names:
            (src / name).write_text(SEND, encoding="utf-8")
        return root

    def _main_over(self, root: Path, sanctioned=None, exempt=None, tracked=None) -> int:
        real = eg.send_sites
        with mock.patch.object(eg, "send_sites", lambda _ignored: real(root)), \
             mock.patch.object(eg, "SANCTIONED", sanctioned or {}), \
             mock.patch.object(eg, "EXEMPT", exempt or {}), \
             mock.patch.object(eg, "TRACKED", tracked or {}), \
             mock.patch.object(eg, "SCANNED_ROOTS", ("crates/organon/src",)):
            return eg.main()

    def test_an_unlisted_send_site_fails(self) -> None:
        """The whole point: a NEW outbound request nobody accounted for."""
        with tempfile.TemporaryDirectory() as tmp:
            root = self._tree(tmp, ["fresh.rs"])
            self.assertEqual(self._main_over(root), 1)

    def test_a_sanctioned_site_passes(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = self._tree(tmp, ["safe.rs"])
            rc = self._main_over(
                root, sanctioned={"crates/organon/src/safe.rs": "is the checkpoint"}
            )
            self.assertEqual(rc, 0)

    def test_an_exempt_site_passes(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = self._tree(tmp, ["cli.rs"])
            rc = self._main_over(
                root, exempt={"crates/organon/src/cli.rs": "operator-supplied destination"}
            )
            self.assertEqual(rc, 0)

    def test_a_tracked_site_passes_while_its_issue_is_open(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = self._tree(tmp, ["known.rs"])
            with mock.patch.object(eg, "open_issue", lambda _n: True):
                rc = self._main_over(root, tracked={"crates/organon/src/known.rs": 1})
            self.assertEqual(rc, 0)

    def test_a_tracked_site_fails_once_its_issue_is_closed(self) -> None:
        """WHY: otherwise closing the issue silently converts a known defect into an
        exemption nobody agreed to."""
        with tempfile.TemporaryDirectory() as tmp:
            root = self._tree(tmp, ["known.rs"])
            with mock.patch.object(eg, "open_issue", lambda _n: False):
                rc = self._main_over(root, tracked={"crates/organon/src/known.rs": 1})
            self.assertEqual(rc, 1)

    def test_a_stale_listing_fails(self) -> None:
        """WHY: a listing for a path with no send call would silently exempt whatever
        occupies that path next."""
        with tempfile.TemporaryDirectory() as tmp:
            root = self._tree(tmp, ["real.rs"])
            rc = self._main_over(
                root,
                exempt={
                    "crates/organon/src/real.rs": "fine",
                    "crates/organon/src/vanished.rs": "no longer exists",
                },
            )
            self.assertEqual(rc, 1)

    def test_unreachable_github_treats_a_tracked_issue_as_open(self) -> None:
        """WHY fail-open here: this check exists to refuse a NEW unrouted site. A
        network hiccup turning the build red would make it the thing people route
        around, and a tracked entry is already a recorded state."""
        with tempfile.TemporaryDirectory() as tmp:
            root = self._tree(tmp, ["known.rs"])
            failed = mock.Mock(returncode=1, stdout=b"")
            with mock.patch.object(eg.subprocess, "run", return_value=failed):
                rc = self._main_over(root, tracked={"crates/organon/src/known.rs": 1})
            self.assertEqual(rc, 0)


if __name__ == "__main__":
    unittest.main()
