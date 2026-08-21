from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path

SCRIPT_PATH = Path(__file__).resolve().parents[1] / "check-schema-descriptions.py"
SPEC = importlib.util.spec_from_file_location("check_schema_descriptions", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {SCRIPT_PATH}")
sd = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = sd
SPEC.loader.exec_module(sd)


class LeakDetection(unittest.TestCase):
    def test_a_field_doc_with_a_tag_is_a_leak(self) -> None:
        """The likelier of the two: field docs become PROPERTY descriptions, which is
        easy to forget because the tag is nowhere near the derive."""
        src = (
            "#[derive(Serialize, JsonSchema)]\n"
            "pub struct P {\n"
            "    /// WHY: clamped because the store panics above 10k.\n"
            "    pub limit: usize,\n"
            "}\n"
        )
        self.assertEqual([n for n, _ in sd.leaking_docs(src)], [3])

    def test_a_type_doc_with_a_tag_is_a_leak(self) -> None:
        src = (
            "/// SECURITY: this carries a bearer token.\n"
            "#[derive(JsonSchema)]\n"
            "pub struct P {}\n"
        )
        self.assertEqual([n for n, _ in sd.leaking_docs(src)], [1])

    def test_a_type_doc_separated_by_attributes_is_still_found(self) -> None:
        """WHY: `#[serde(...)]` commonly sits between the docs and the derive, and a
        reader that stopped at the first non-doc line would miss every such type."""
        src = (
            "/// WARNING: do not reorder these variants.\n"
            "#[serde(rename_all = \"camelCase\")]\n"
            "#[derive(JsonSchema)]\n"
            "pub struct P {}\n"
        )
        self.assertEqual([n for n, _ in sd.leaking_docs(src)], [1])

    def test_an_ordinary_description_is_not_a_leak(self) -> None:
        """The guard must not fire on the text these comments are FOR."""
        src = (
            "/// Parameters for a memory search.\n"
            "#[derive(JsonSchema)]\n"
            "pub struct P {\n"
            "    /// Free-text query, matched via BM25 against current fact content.\n"
            "    pub query: String,\n"
            "}\n"
        )
        self.assertEqual(sd.leaking_docs(src), [])

    def test_a_tag_word_inside_a_sentence_is_not_a_leak(self) -> None:
        """WHY whole-word matching: prose legitimately contains these words, and a
        substring match would fire on 'note', 'nothing', 'noteworthy'."""
        src = (
            "#[derive(JsonSchema)]\n"
            "pub struct P {\n"
            "    /// Nothing is returned when the topic is unknown.\n"
            "    pub topic: String,\n"
            "}\n"
        )
        self.assertEqual(sd.leaking_docs(src), [])

    def test_a_plain_comment_inside_the_item_is_not_a_leak(self) -> None:
        """`//` is not published. Moving the note there is the fix the message asks
        for, so it must not itself be a finding."""
        src = (
            "#[derive(JsonSchema)]\n"
            "pub struct P {\n"
            "    // WHY: clamped because the store panics above 10k.\n"
            "    pub limit: usize,\n"
            "}\n"
        )
        self.assertEqual(sd.leaking_docs(src), [])

    def test_a_tag_on_a_type_without_JsonSchema_is_not_a_leak(self) -> None:
        """The guard is about published API, not about comments in general."""
        src = (
            "/// WHY: internal bookkeeping only.\n"
            "#[derive(Debug)]\n"
            "pub struct P {\n"
            "    /// SAFETY: never exposed.\n"
            "    pub x: usize,\n"
            "}\n"
        )
        self.assertEqual(sd.leaking_docs(src), [])

    def test_the_scan_stops_at_the_end_of_the_item(self) -> None:
        """WHY pinned: a scan that ran past the closing brace would attribute the next
        type's comments to this one and report a leak in a file with none."""
        src = (
            "#[derive(JsonSchema)]\n"
            "pub struct P {\n"
            "    pub x: usize,\n"
            "}\n"
            "\n"
            "/// WHY: this type is not schema-bearing.\n"
            "pub struct Q;\n"
        )
        self.assertEqual(sd.leaking_docs(src), [])


class ThisRepository(unittest.TestCase):
    def test_the_tree_publishes_no_maintainer_notes(self) -> None:
        self.assertEqual(sd.main(), 0)

    def test_the_reader_still_finds_schema_items(self) -> None:
        """WHY: a reader that had stopped matching `JsonSchema` would report a clean
        result forever, and every test above would still pass."""
        seen = 0
        for path in sd.tracked_rust_files(sd.REPO_ROOT):
            try:
                seen += len(sd.DERIVE_JSON_SCHEMA.findall(path.read_text(encoding="utf-8")))
            except (OSError, UnicodeError):
                continue
        self.assertGreater(seen, 0, "no JsonSchema derives found; the reader is broken")


if __name__ == "__main__":
    unittest.main()
