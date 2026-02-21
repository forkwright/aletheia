"""Entity resolution — canonical registry with fuzzy matching.

Prevents duplicate entity nodes in Neo4j by maintaining a canonical name
registry and resolving new entities against it before creation.
"""

import logging
import re
from difflib import SequenceMatcher

from .graph import neo4j_driver, neo4j_available, mark_neo4j_ok, mark_neo4j_down

logger = logging.getLogger("aletheia_memory.entity_resolver")

# Manual alias table — known equivalences from corpus audit
KNOWN_ALIASES: dict[str, str] = {
    "aletheia_runtime": "aletheia",
    "aletheia_system": "aletheia",
    "aletheia system": "aletheia",
    "the aletheia system": "aletheia",
    "cody kickertz": "cody",
    "cody craft": "cody",
    "ck": "cody",
    "kendall_identity": "kendall",
    "1997_ram_2500": "1997 ram 2500",
    "1997 dodge ram": "1997 ram 2500",
    "ram 2500": "1997 ram 2500",
    "ut_mccombs": "ut mccombs",
    "ut austin": "ut mccombs",
    "mccombs": "ut mccombs",
    "summus_global": "summus",
    "summus global": "summus",
    "ardent_llc": "ardent",
    "ardent llc": "ardent",
}

# Stopword entities — never create nodes for these
STOPWORDS = {
    "the", "a", "an", "is", "was", "are", "were", "be", "been", "being",
    "have", "has", "had", "do", "does", "did", "will", "would", "could",
    "should", "may", "might", "shall", "can", "must", "that", "this",
    "these", "those", "it", "its", "they", "them", "their", "we", "our",
    "you", "your", "he", "his", "she", "her", "if", "then", "else",
    "when", "where", "how", "what", "which", "who", "whom", "why",
    "not", "no", "yes", "ok", "done", "true", "false", "null", "none",
    "just", "also", "very", "too", "only", "even", "still", "already",
    "system", "user", "agent", "tool", "command", "output", "input",
    "result", "error", "warning", "info", "debug", "log", "data",
    "file", "path", "name", "type", "value", "key", "id", "status",
    "ping", "pong", "convo", "conversation", "session", "turn",
    "message", "response", "request", "query", "search",
}

# Minimum entity name length
MIN_NAME_LENGTH = 2
MAX_NAME_LENGTH = 100

# Fuzzy match threshold (0-1, higher = stricter)
FUZZY_THRESHOLD = 0.85


def normalize_entity_name(name: str) -> str:
    """Normalize an entity name for comparison."""
    s = name.strip().lower()
    # Remove common prefixes
    s = re.sub(r'^(the|a|an)\s+', '', s)
    # Collapse whitespace
    s = re.sub(r'\s+', ' ', s)
    # Remove trailing punctuation
    s = s.rstrip('.,;:!?')
    return s


def is_valid_entity(name: str) -> bool:
    """Check if a name is a valid entity (not a stopword, not too short/long)."""
    normalized = normalize_entity_name(name)
    if len(normalized) < MIN_NAME_LENGTH or len(normalized) > MAX_NAME_LENGTH:
        return False
    if normalized in STOPWORDS:
        return False
    # Pure numbers are not entities
    if re.match(r'^\d+$', normalized):
        return False
    # Single common words
    if len(normalized.split()) == 1 and normalized in STOPWORDS:
        return False
    return True


def resolve_entity(name: str, existing_names: list[str] | None = None) -> str | None:
    """Resolve an entity name to its canonical form.
    
    Returns:
        Canonical name if resolved, None if the entity should be skipped.
    """
    normalized = normalize_entity_name(name)
    
    if not is_valid_entity(name):
        return None
    
    # Check known aliases first
    if normalized in KNOWN_ALIASES:
        return KNOWN_ALIASES[normalized]
    
    # If we have existing names, try fuzzy matching
    if existing_names:
        for existing in existing_names:
            existing_norm = normalize_entity_name(existing)
            ratio = SequenceMatcher(None, normalized, existing_norm).ratio()
            if ratio >= FUZZY_THRESHOLD:
                return existing_norm
    
    return normalized


def get_canonical_entities() -> list[str]:
    """Fetch all canonical entity names from Neo4j."""
    if not neo4j_available():
        return []
    
    try:
        driver = neo4j_driver()
        with driver.session() as session:
            result = session.run(
                "MATCH (e) WHERE e:Entity OR e:__Entity__ "
                "RETURN DISTINCT toLower(e.name) AS name"
            )
            names = [r["name"] for r in result if r["name"]]
        driver.close()
        mark_neo4j_ok()
        return names
    except Exception:
        mark_neo4j_down()
        logger.warning("Failed to fetch canonical entities", exc_info=True)
        return []


def merge_duplicate_entities() -> dict:
    """Find and merge duplicate entity nodes in Neo4j.
    
    Uses the alias table and fuzzy matching to identify duplicates,
    then merges relationships onto the canonical node and deletes duplicates.
    """
    if not neo4j_available():
        return {"ok": False, "reason": "neo4j_unavailable"}
    
    try:
        driver = neo4j_driver()
        merged_count = 0
        deleted_count = 0
        
        with driver.session() as session:
            # Get all entities
            result = session.run(
                "MATCH (e) WHERE e:Entity OR e:__Entity__ "
                "RETURN e.name AS name, id(e) AS node_id"
            )
            entities = [(r["name"], r["node_id"]) for r in result if r["name"]]
        
        # Build resolution map
        canonical_map: dict[str, list[tuple[str, int]]] = {}
        for name, node_id in entities:
            resolved = resolve_entity(name)
            if resolved is None:
                # Invalid entity — mark for deletion
                with driver.session() as session:
                    session.run(
                        "MATCH (e) WHERE id(e) = $node_id DETACH DELETE e",
                        node_id=node_id,
                    )
                    deleted_count += 1
                continue
            
            if resolved not in canonical_map:
                canonical_map[resolved] = []
            canonical_map[resolved].append((name, node_id))
        
        # Merge duplicates
        for canonical, nodes in canonical_map.items():
            if len(nodes) <= 1:
                continue
            
            # Keep the first node as canonical, merge others into it
            keeper_name, keeper_id = nodes[0]
            
            with driver.session() as session:
                for dup_name, dup_id in nodes[1:]:
                    # Transfer all relationships from duplicate to keeper
                    session.run(
                        """
                        MATCH (dup) WHERE id(dup) = $dup_id
                        MATCH (keeper) WHERE id(keeper) = $keeper_id
                        OPTIONAL MATCH (dup)-[r]->(target)
                        WHERE target <> keeper
                        WITH keeper, dup, collect({type: type(r), target: target, props: properties(r)}) AS outRels
                        UNWIND outRels AS rel
                        WITH keeper, dup, rel
                        WHERE rel.target IS NOT NULL
                        CALL apoc.create.relationship(keeper, rel.type, rel.props, rel.target) YIELD rel AS newRel
                        RETURN count(newRel)
                        """,
                        dup_id=dup_id, keeper_id=keeper_id,
                    )
                    # Also transfer incoming relationships
                    session.run(
                        """
                        MATCH (dup) WHERE id(dup) = $dup_id
                        MATCH (keeper) WHERE id(keeper) = $keeper_id
                        OPTIONAL MATCH (source)-[r]->(dup)
                        WHERE source <> keeper
                        WITH keeper, dup, collect({type: type(r), source: source, props: properties(r)}) AS inRels
                        UNWIND inRels AS rel
                        WITH keeper, dup, rel
                        WHERE rel.source IS NOT NULL
                        CALL apoc.create.relationship(rel.source, rel.type, rel.props, keeper) YIELD rel AS newRel
                        RETURN count(newRel)
                        """,
                        dup_id=dup_id, keeper_id=keeper_id,
                    )
                    # Delete the duplicate
                    session.run(
                        "MATCH (e) WHERE id(e) = $dup_id DETACH DELETE e",
                        dup_id=dup_id,
                    )
                    merged_count += 1
                
                # Update keeper to canonical name
                session.run(
                    "MATCH (e) WHERE id(e) = $keeper_id SET e.name = $canonical",
                    keeper_id=keeper_id, canonical=canonical,
                )
        
        driver.close()
        mark_neo4j_ok()
        
        result = {
            "ok": True,
            "entities_checked": len(entities),
            "duplicates_merged": merged_count,
            "invalid_deleted": deleted_count,
            "canonical_groups": len(canonical_map),
        }
        logger.info(f"Entity merge: {result}")
        return result
        
    except Exception as e:
        mark_neo4j_down()
        logger.exception("Entity merge failed")
        return {"ok": False, "error": str(e)}


def cleanup_orphan_entities() -> dict:
    """Remove entity nodes with no relationships."""
    if not neo4j_available():
        return {"ok": False, "reason": "neo4j_unavailable"}
    
    try:
        driver = neo4j_driver()
        with driver.session() as session:
            result = session.run(
                "MATCH (e) WHERE (e:Entity OR e:__Entity__) AND NOT (e)--() "
                "WITH e LIMIT 500 "
                "DELETE e "
                "RETURN count(e) AS deleted"
            )
            deleted = result.single()["deleted"]
        driver.close()
        mark_neo4j_ok()
        return {"ok": True, "orphans_deleted": deleted}
    except Exception as e:
        mark_neo4j_down()
        return {"ok": False, "error": str(e)}
