# Tests for neo4j driver version requirements and import compatibility

import re
from pathlib import Path

import pytest

PYPROJECT_PATH = Path(__file__).parent.parent / "pyproject.toml"
GRAPH_PY_PATH = Path(__file__).parent.parent / "aletheia_memory" / "graph.py"


def test_pyproject_has_neo4j_6x():
    """pyproject.toml must declare neo4j>=6.1.0."""
    content = PYPROJECT_PATH.read_text()
    assert "neo4j>=6.1.0" in content, (
        f"Expected 'neo4j>=6.1.0' in pyproject.toml, got: "
        f"{[l for l in content.splitlines() if 'neo4j' in l]}"
    )


def test_pyproject_no_neo4j_driver():
    """pyproject.toml must NOT reference the deprecated neo4j-driver package."""
    content = PYPROJECT_PATH.read_text()
    assert "neo4j-driver" not in content, (
        "Found 'neo4j-driver' in pyproject.toml — use 'neo4j>=6.1.0' instead"
    )


def test_pyproject_has_neo4j_graphrag():
    """pyproject.toml must declare neo4j-graphrag[anthropic]>=1.13.0."""
    content = PYPROJECT_PATH.read_text()
    assert "neo4j-graphrag[anthropic]>=1.13.0" in content, (
        f"Expected 'neo4j-graphrag[anthropic]>=1.13.0' in pyproject.toml, got: "
        f"{[l for l in content.splitlines() if 'graphrag' in l]}"
    )


def test_neo4j_graphdatabase_import():
    """neo4j package must be importable as GraphDatabase (not neo4j-driver).

    Skipped when neo4j is not installed in the local environment — the package
    lives on the server. pyproject.toml version check covers the spec constraint.
    """
    neo4j = pytest.importorskip("neo4j", reason="neo4j not installed locally — server-side dep")
    assert hasattr(neo4j, "GraphDatabase")


def test_graph_py_uses_neo4j_not_driver():
    """graph.py must import from 'neo4j', not 'neo4j-driver' or 'neo4j_driver' packages."""
    content = GRAPH_PY_PATH.read_text()
    assert "from neo4j import" in content, (
        "graph.py must use 'from neo4j import ...'"
    )
    # Check no import of the deprecated neo4j-driver package (import would be
    # 'import neo4j_driver' or 'from neo4j_driver import ...')
    assert not re.search(r"import neo4j_driver", content), (
        "graph.py must not import from 'neo4j_driver' (deprecated package name)"
    )
    assert "neo4j-driver" not in content, (
        "graph.py must not reference 'neo4j-driver' pip package name"
    )
