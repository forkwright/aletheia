# Neo4j driver management and cached availability check
import logging
import threading
import time
from typing import Any

import neo4j

from .config import NEO4J_PASSWORD, NEO4J_URL, NEO4J_USER

logger = logging.getLogger("aletheia_memory.graph")

_neo4j_ok: bool | None = None
_neo4j_checked_at: float = 0.0
_check_lock = threading.Lock()
_CHECK_INTERVAL = 30.0  # seconds

# Shared driver instance, set from app lifespan
_shared_driver: neo4j.Driver | None = None


def set_shared_driver(driver: neo4j.Driver | None) -> None:
    """Set the shared Neo4j driver (called from app lifespan)."""
    global _shared_driver
    _shared_driver = driver


def neo4j_driver() -> neo4j.Driver:
    """Return the shared Neo4j driver, or create a fresh one if not set."""
    if _shared_driver is not None:
        return _shared_driver
    from neo4j import GraphDatabase
    driver: neo4j.Driver = GraphDatabase.driver(NEO4J_URL, auth=(NEO4J_USER, NEO4J_PASSWORD))  # pyright: ignore[reportUnknownMemberType]
    return driver


def neo4j_available() -> bool:
    """Check if Neo4j is configured and reachable. Result cached for 30s."""
    global _neo4j_ok, _neo4j_checked_at

    if not NEO4J_PASSWORD:
        return False

    now = time.monotonic()
    if _neo4j_ok is not None and (now - _neo4j_checked_at) < _CHECK_INTERVAL:
        return _neo4j_ok

    with _check_lock:
        now = time.monotonic()
        if _neo4j_ok is not None and (now - _neo4j_checked_at) < _CHECK_INTERVAL:
            return _neo4j_ok

        try:
            driver = neo4j_driver()
            driver.verify_connectivity()  # pyright: ignore[reportUnknownMemberType]
            _neo4j_ok = True
        except Exception:
            _neo4j_ok = False
        _neo4j_checked_at = now
        return _neo4j_ok


def mark_neo4j_ok():
    """Mark Neo4j as available after a successful operation."""
    global _neo4j_ok, _neo4j_checked_at
    _neo4j_ok = True
    _neo4j_checked_at = time.monotonic()


def mark_neo4j_down():
    """Mark Neo4j as unavailable after a failed operation."""
    global _neo4j_ok, _neo4j_checked_at
    _neo4j_ok = False
    _neo4j_checked_at = time.monotonic()


GRAPH_UNAVAILABLE: dict[str, Any] = {"ok": False, "available": False, "reason": "graph_unavailable"}
