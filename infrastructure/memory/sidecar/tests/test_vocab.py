# Tests for vocabulary loading, normalization, and RELATES_TO elimination

import json
import tempfile
from pathlib import Path
from unittest.mock import patch

import pytest

from aletheia_memory import vocab as vocab_module
from aletheia_memory.vocab import CONTROLLED_VOCAB, normalize_type, load_vocab
from aletheia_memory.config import GRAPH_EXTRACTION_PROMPT


# ---------------------------------------------------------------------------
# normalize_type — known types
# ---------------------------------------------------------------------------


def test_normalize_known_type():
    """Exact match in CONTROLLED_VOCAB passes through unchanged."""
    assert normalize_type("KNOWS") == "KNOWS"


def test_normalize_known_type_lives_in():
    """LIVES_IN passes through unchanged."""
    assert normalize_type("LIVES_IN") == "LIVES_IN"


def test_normalize_lowercase_mapping():
    """Lowercase alias 'works_at' maps to WORKS_AT via TYPE_MAP."""
    assert normalize_type("works_at") == "WORKS_AT"


def test_normalize_with_spaces():
    """Space-separated alias is normalized to underscores before lookup."""
    assert normalize_type("member of") == "MEMBER_OF"


def test_normalize_with_hyphens():
    """Hyphenated alias normalizes to underscores before TYPE_MAP lookup."""
    assert normalize_type("works-at") == "WORKS_AT"


# ---------------------------------------------------------------------------
# normalize_type — keyword matching
# ---------------------------------------------------------------------------


def test_normalize_keyword_match_knows():
    """Partial keyword 'know' maps 'knowledge' to KNOWS."""
    assert normalize_type("knowledge") == "KNOWS"


def test_normalize_keyword_match_studies():
    """Partial keyword 'stud' maps 'studying' to STUDIES."""
    assert normalize_type("studying_at") == "STUDIES"


# ---------------------------------------------------------------------------
# normalize_type — unknown returns None
# ---------------------------------------------------------------------------


def test_normalize_unknown_returns_none():
    """Completely unknown type returns None — not a fallback type."""
    result = normalize_type("FOOBAR_NONSENSE")
    assert result is None


def test_normalize_never_returns_relates_to_for_is():
    """'is' was previously mapped to RELATES_TO — now must return None."""
    result = normalize_type("is")
    assert result != "RELATES_TO", "'is' must not map to RELATES_TO"
    assert result is None, f"Expected None, got {result!r}"


def test_normalize_never_returns_relates_to_for_is_a():
    """'is_a' was previously mapped to RELATES_TO — now must return None."""
    result = normalize_type("is_a")
    assert result != "RELATES_TO"
    assert result is None


def test_normalize_never_returns_relates_to_for_relates_to():
    """'relates_to' was previously mapped to RELATES_TO — now must return None."""
    result = normalize_type("relates_to")
    assert result != "RELATES_TO"
    assert result is None


# ---------------------------------------------------------------------------
# CONTROLLED_VOCAB — RELATES_TO must not exist
# ---------------------------------------------------------------------------


def test_relates_to_not_in_vocab():
    """RELATES_TO must be absent from CONTROLLED_VOCAB."""
    assert "RELATES_TO" not in CONTROLLED_VOCAB


def test_controlled_vocab_is_frozenset():
    """CONTROLLED_VOCAB is a frozenset (immutable, set semantics)."""
    assert isinstance(CONTROLLED_VOCAB, frozenset)


def test_controlled_vocab_has_expected_types():
    """Core relationship types are present in CONTROLLED_VOCAB."""
    required = {"KNOWS", "LIVES_IN", "WORKS_AT", "OWNS", "USES", "PREFERS"}
    assert required.issubset(CONTROLLED_VOCAB)


# ---------------------------------------------------------------------------
# load_vocab — file loading
# ---------------------------------------------------------------------------


def test_load_vocab_from_file():
    """load_vocab reads relationship_types from JSON file when present."""
    custom_types = ["FRIENDS_WITH", "ENEMY_OF", "ALLIED_WITH"]
    vocab_data = {
        "version": 1,
        "relationship_types": custom_types,
        "fallback_type": None,
        "normalization_log": True,
    }

    with tempfile.NamedTemporaryFile(
        mode="w", suffix=".json", delete=False
    ) as f:
        json.dump(vocab_data, f)
        tmp_path = Path(f.name)

    try:
        with patch.object(vocab_module, "_VOCAB_PATH", tmp_path):
            result = load_vocab()
    finally:
        tmp_path.unlink(missing_ok=True)

    assert isinstance(result, frozenset)
    assert "FRIENDS_WITH" in result
    assert "ENEMY_OF" in result
    assert "ALLIED_WITH" in result


def test_load_vocab_uppercases_types_from_file():
    """load_vocab uppercases relationship types from the JSON file."""
    vocab_data = {"version": 1, "relationship_types": ["friends_with", "knows"]}

    with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
        json.dump(vocab_data, f)
        tmp_path = Path(f.name)

    try:
        with patch.object(vocab_module, "_VOCAB_PATH", tmp_path):
            result = load_vocab()
    finally:
        tmp_path.unlink(missing_ok=True)

    assert "FRIENDS_WITH" in result
    assert "KNOWS" in result


def test_load_vocab_fallback_on_missing_file():
    """load_vocab returns hardcoded defaults when vocab file does not exist."""
    missing = Path("/tmp/nonexistent_vocab_file_xyz.json")
    with patch.object(vocab_module, "_VOCAB_PATH", missing):
        result = load_vocab()
    assert isinstance(result, frozenset)
    assert "KNOWS" in result
    assert "RELATES_TO" not in result


def test_load_vocab_fallback_on_corrupt_file():
    """load_vocab returns hardcoded defaults when JSON is invalid."""
    with tempfile.NamedTemporaryFile(
        mode="w", suffix=".json", delete=False
    ) as f:
        f.write("this is not valid JSON {{{{")
        tmp_path = Path(f.name)

    try:
        with patch.object(vocab_module, "_VOCAB_PATH", tmp_path):
            result = load_vocab()
    finally:
        tmp_path.unlink(missing_ok=True)

    assert isinstance(result, frozenset)
    assert "KNOWS" in result


def test_load_vocab_fallback_on_empty_types():
    """load_vocab falls back to defaults when relationship_types is empty."""
    vocab_data = {"version": 1, "relationship_types": []}

    with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
        json.dump(vocab_data, f)
        tmp_path = Path(f.name)

    try:
        with patch.object(vocab_module, "_VOCAB_PATH", tmp_path):
            result = load_vocab()
    finally:
        tmp_path.unlink(missing_ok=True)

    assert "KNOWS" in result


# ---------------------------------------------------------------------------
# GRAPH_EXTRACTION_PROMPT — no RELATES_TO reference
# ---------------------------------------------------------------------------


def test_graph_extraction_prompt_no_relates_to():
    """GRAPH_EXTRACTION_PROMPT must not mention RELATES_TO."""
    assert "RELATES_TO" not in GRAPH_EXTRACTION_PROMPT, (
        "GRAPH_EXTRACTION_PROMPT must not instruct LLM to use RELATES_TO as fallback"
    )


def test_graph_extraction_prompt_instructs_skip():
    """GRAPH_EXTRACTION_PROMPT must tell the LLM to skip unmatched relationships."""
    assert "skip" in GRAPH_EXTRACTION_PROMPT.lower(), (
        "Prompt should instruct LLM to skip relationships that don't match vocab"
    )
