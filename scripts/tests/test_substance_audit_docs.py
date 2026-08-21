from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT_PATH = Path(__file__).resolve().parents[1] / "substance-audit.py"
SPEC = importlib.util.spec_from_file_location("substance_audit", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {SCRIPT_PATH}")
sa = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = sa
SPEC.loader.exec_module(sa)


def scan(source: str) -> list[str]:
    """Run the doc scan over one synthetic crate and return the flagged lines."""
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        src = root / "crates" / "probe" / "src"
        src.mkdir(parents=True)
        (src / "lib.rs").write_text(source, encoding="utf-8")
        findings, errors = sa.scan_tautological_docs(root, "crates/probe")
        assert not errors, errors
        return [f["text"] for f in findings]


class TautologicalDocScan(unittest.TestCase):
    """The scan must test whether a doc restates its item, not whether it opens
    with a particular phrase.

    WHY these cases: run as a bare prefix match the scan reported 116 findings
    across episteme, krites and nous, of which at most a handful restated
    anything. The release process turns every advisory class into an owning-issue
    obligation, so the false positives were not merely noise -- they manufactured
    release work out of documentation written the way the project requires.
    """

    def test_flags_a_doc_that_only_restates_the_item_name(self) -> None:
        flagged = scan("/// Get the agent name.\npub fn agent_name(&self) -> &str { \"\" }\n")
        self.assertEqual(flagged, ["/// Get the agent name."])

    def test_flags_across_an_intervening_attribute(self) -> None:
        source = "/// Sets the timeout.\n#[must_use]\npub fn timeout(mut self) -> Self { self }\n"
        self.assertEqual(flagged := scan(source), ["/// Sets the timeout."], flagged)

    def test_abbreviated_identifier_still_counts_as_a_restatement(self) -> None:
        """`agent_config` documented as "the agent configuration" adds nothing."""
        source = "/// Get the agent configuration.\npub fn agent_config(&self) -> u8 { 0 }\n"
        self.assertEqual(scan(source), ["/// Get the agent configuration."])

    def test_ignores_a_doc_that_adds_information(self) -> None:
        source = (
            "/// Returns the filename stem, or the last two path components for deeper paths.\n"
            "pub fn short_name(&self) -> String { String::new() }\n"
        )
        self.assertEqual(scan(source), [])

    def test_ignores_an_errors_section(self) -> None:
        """rustdoc renders `# Errors` as "Returns <error> if <condition>".

        Flagging that reports conformance with the project's own documentation
        requirement as a defect.
        """
        source = (
            "/// # Errors\n"
            "///\n"
            "/// Returns the error when the socket cannot bind.\n"
            "pub fn bind(&self) -> Result<(), ()> { Ok(()) }\n"
        )
        self.assertEqual(scan(source), [])

    def test_ignores_a_doc_naming_a_concrete_type(self) -> None:
        source = "/// Returns a [`Receiver`] of events.\npub fn events(&self) -> u8 { 0 }\n"
        self.assertEqual(scan(source), [])

    def test_ignores_a_stated_condition(self) -> None:
        source = (
            "/// Returns the cached value if it is still fresh.\n"
            "pub fn value(&self) -> u8 { 0 }\n"
        )
        self.assertEqual(scan(source), [])


if __name__ == "__main__":
    unittest.main()
