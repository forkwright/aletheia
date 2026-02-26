# Controlled relationship type vocabulary for Neo4j graph store

from __future__ import annotations

import json
import logging
from pathlib import Path
from typing import Any, cast

log = logging.getLogger("aletheia_memory.vocab")

_VOCAB_PATH = Path.home() / ".aletheia" / "graph_vocab.json"

_HARDCODED_VOCAB: frozenset[str] = frozenset({
    "KNOWS", "LIVES_IN", "WORKS_AT", "OWNS", "USES", "PREFERS",
    "STUDIES", "MANAGES", "MEMBER_OF", "INTERESTED_IN", "SKILLED_IN",
    "CREATED", "MAINTAINS", "DEPENDS_ON", "LOCATED_IN", "PART_OF",
    "SCHEDULED_FOR", "DIAGNOSED_WITH", "PRESCRIBED", "TREATS",
    "VEHICLE_IS", "INSTALLED_ON", "COMPATIBLE_WITH", "CONNECTED_TO",
    "COMMUNICATES_VIA", "CONFIGURED_WITH", "RUNS_ON", "SERVES",
})


def load_vocab() -> frozenset[str]:
    """Load controlled vocabulary from ~/.aletheia/graph_vocab.json.

    Expected JSON structure:
        {"version": 1, "relationship_types": [...], "fallback_type": null, "normalization_log": true}

    Falls back to hardcoded defaults if the file is absent or unparseable.
    """
    try:
        raw = _VOCAB_PATH.read_text(encoding="utf-8")
        data = json.loads(raw)
        types: Any = data.get("relationship_types", [])
        if not isinstance(types, list) or not types:
            raise ValueError("relationship_types must be a non-empty list")
        type_list = cast("list[str]", types)
        vocab = frozenset(t.upper() for t in type_list)
        log.debug("loaded %d relationship types from %s", len(vocab), _VOCAB_PATH)
        return vocab
    except FileNotFoundError:
        log.debug("vocab file not found at %s — using hardcoded defaults", _VOCAB_PATH)
        return _HARDCODED_VOCAB
    except Exception as exc:
        log.warning("failed to load vocab from %s (%s) — using hardcoded defaults", _VOCAB_PATH, exc)
        return _HARDCODED_VOCAB


CONTROLLED_VOCAB: frozenset[str] = load_vocab()

TYPE_MAP: dict[str, str] = {
    "has": "OWNS",
    "is_part_of": "PART_OF",
    "part_of": "PART_OF",
    "works_at": "WORKS_AT",
    "works_on": "WORKS_AT",
    "lives_in": "LIVES_IN",
    "located_in": "LOCATED_IN",
    "located_at": "LOCATED_IN",
    "uses": "USES",
    "used_by": "USES",
    "used_for": "USES",
    "runs_on": "RUNS_ON",
    "runs": "RUNS_ON",
    "depends_on": "DEPENDS_ON",
    "requires": "DEPENDS_ON",
    "knows": "KNOWS",
    "knows_about": "KNOWS",
    "knows_of": "KNOWS",
    "prefers": "PREFERS",
    "likes": "PREFERS",
    "interested_in": "INTERESTED_IN",
    "studies": "STUDIES",
    "studying": "STUDIES",
    "created": "CREATED",
    "created_by": "CREATED",
    "built": "CREATED",
    "made": "CREATED",
    "maintains": "MAINTAINS",
    "managed_by": "MANAGES",
    "manages": "MANAGES",
    "member_of": "MEMBER_OF",
    "belongs_to": "MEMBER_OF",
    "skilled_in": "SKILLED_IN",
    "skilled_at": "SKILLED_IN",
    "owns": "OWNS",
    "has_a": "OWNS",
    "installed_on": "INSTALLED_ON",
    "installed": "INSTALLED_ON",
    "compatible_with": "COMPATIBLE_WITH",
    "connected_to": "CONNECTED_TO",
    "communicates_via": "COMMUNICATES_VIA",
    "configured_with": "CONFIGURED_WITH",
    "serves": "SERVES",
    "diagnosed_with": "DIAGNOSED_WITH",
    "prescribed": "PRESCRIBED",
    "treats": "TREATS",
    "scheduled_for": "SCHEDULED_FOR",
    "vehicle_is": "VEHICLE_IS",
}

KEYWORD_MAP: dict[str, str] = {
    "know": "KNOWS",
    "live": "LIVES_IN",
    "work": "WORKS_AT",
    "own": "OWNS",
    "use": "USES",
    "prefer": "PREFERS",
    "stud": "STUDIES",
    "manag": "MANAGES",
    "member": "MEMBER_OF",
    "interest": "INTERESTED_IN",
    "skill": "SKILLED_IN",
    "creat": "CREATED",
    "maintain": "MAINTAINS",
    "depend": "DEPENDS_ON",
    "locat": "LOCATED_IN",
    "part": "PART_OF",
    "schedul": "SCHEDULED_FOR",
    "diagnos": "DIAGNOSED_WITH",
    "prescri": "PRESCRIBED",
    "treat": "TREATS",
    "vehicle": "VEHICLE_IS",
    "install": "INSTALLED_ON",
    "compat": "COMPATIBLE_WITH",
    "connect": "CONNECTED_TO",
    "communic": "COMMUNICATES_VIA",
    "config": "CONFIGURED_WITH",
    "run": "RUNS_ON",
    "serv": "SERVES",
}


def normalize_type(rel_type: str) -> str | None:
    """Map a relationship type to controlled vocabulary.

    Returns None if no match is found. Callers decide how to handle unknowns —
    typically by skipping the relationship rather than falling back to a vague type.
    """
    if rel_type in CONTROLLED_VOCAB:
        return rel_type

    lower = rel_type.lower().strip()
    if lower in TYPE_MAP:
        return TYPE_MAP[lower]

    normalized = lower.replace(" ", "_").replace("-", "_")
    if normalized in TYPE_MAP:
        return TYPE_MAP[normalized]

    for keyword, target in KEYWORD_MAP.items():
        if keyword in normalized:
            return target

    return None
