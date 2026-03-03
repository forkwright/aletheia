# INVARIANT TESTS — design decisions that must not regress.
#
# These complement test_vocab.py with broader structural invariants.
# They run in the pre-commit hook and CI. If one fails, a design
# decision was violated — read the assertion before "fixing" it.

from aletheia_memory.config import GRAPH_EXTRACTION_PROMPT
from aletheia_memory.vocab import CONTROLLED_VOCAB, normalize_type

# ---------------------------------------------------------------------------
# RELATES_TO elimination — the foundational invariant
# ---------------------------------------------------------------------------


def test_relates_to_absent_from_vocab():
    """RELATES_TO was eliminated from the vocabulary. It must never return."""
    assert "RELATES_TO" not in CONTROLLED_VOCAB


def test_relates_to_absent_from_prompt():
    """The extraction prompt must not mention RELATES_TO in any form."""
    assert "RELATES_TO" not in GRAPH_EXTRACTION_PROMPT
    assert "relates_to" not in GRAPH_EXTRACTION_PROMPT.lower()


def test_normalize_never_produces_relates_to():
    """No input to normalize_type() should ever produce RELATES_TO."""
    suspicious_inputs = [
        "relates_to", "RELATES_TO", "relates to", "related_to",
        "is", "is_a", "has", "of", "generic", "other", "misc",
    ]
    for inp in suspicious_inputs:
        result = normalize_type(inp)
        assert result != "RELATES_TO", f"normalize_type({inp!r}) returned RELATES_TO"


# ---------------------------------------------------------------------------
# Vocabulary structure
# ---------------------------------------------------------------------------


def test_vocab_is_immutable():
    """CONTROLLED_VOCAB must be a frozenset — no runtime mutation."""
    assert isinstance(CONTROLLED_VOCAB, frozenset)


def test_vocab_all_uppercase():
    """All vocabulary entries must be UPPER_SNAKE_CASE."""
    for entry in CONTROLLED_VOCAB:
        assert entry == entry.upper(), f"Vocab entry {entry!r} is not uppercase"
        assert " " not in entry, f"Vocab entry {entry!r} contains spaces"


def test_vocab_minimum_types():
    """Core relationship types must always be present."""
    required = {"KNOWS", "LIVES_IN", "WORKS_AT", "OWNS", "USES", "PREFERS"}
    missing = required - CONTROLLED_VOCAB
    assert not missing, f"Missing required vocab types: {missing}"


# ---------------------------------------------------------------------------
# Extraction prompt
# ---------------------------------------------------------------------------


def test_prompt_instructs_skip_on_mismatch():
    """The prompt must tell the LLM to skip relationships that don't match."""
    lower = GRAPH_EXTRACTION_PROMPT.lower()
    assert "skip" in lower or "omit" in lower or "do not" in lower, (
        "Prompt must instruct LLM to skip/omit unmatched relationships"
    )
