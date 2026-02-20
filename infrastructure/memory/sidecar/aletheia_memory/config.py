"""Mem0 configuration with auto-detecting LLM backend."""

import logging
import os

from .llm_backend import detect_backend

log = logging.getLogger("aletheia.memory.config")

QDRANT_HOST = os.environ.get("QDRANT_HOST", "localhost")
QDRANT_PORT = int(os.environ.get("QDRANT_PORT", "6333"))
NEO4J_URL = os.environ.get("NEO4J_URL", "neo4j://localhost:7687")
NEO4J_USER = os.environ.get("NEO4J_USER", "neo4j")
NEO4J_PASSWORD = os.environ.get("NEO4J_PASSWORD", os.environ.get("NEO4J_PASS", "chiron-memory"))
VOYAGE_API_KEY = os.environ.get("VOYAGE_API_KEY", "")

FACT_EXTRACTION_PROMPT = """\
You extract durable personal facts from conversations. Output JSON only.

EXTRACT -- lasting facts about the user:
- Identity: name, age, location, family, relationships
- Health: diagnoses, medications, conditions, providers
- Preferences: tools, workflows, food, communication style
- Skills: programming languages, technical abilities, certifications
- Possessions: vehicles, devices, property, subscriptions
- Work: employer, role, projects, colleagues
- Education: degrees, courses, institutions
- Interests: hobbies, goals, values

DO NOT EXTRACT:
- Ongoing tasks ("currently deploying", "working on a bug")
- Debugging sessions or troubleshooting steps
- Tool outputs, error messages, or log snippets
- Transient states ("server is down", "just restarted")
- Conversational filler ("sure", "let me check")
- Facts about the AI assistant itself
- Information already implied by previous context

QUALITY RULES:
- Each fact must stand alone without session context
- Use the user's actual name when known, not "the user"
- Prefer specific over vague ("drives a 2024 4Runner" not "has a vehicle")
- One fact per entry, no compound sentences
- Skip if uncertain -- fewer quality memories beats many poor ones

Output format:
{"facts": ["fact one", "fact two"]}

Return {"facts": []} if nothing worth extracting."""

GRAPH_EXTRACTION_PROMPT = (
    "Use ONLY the following relationship types: "
    "KNOWS, LIVES_IN, WORKS_AT, OWNS, USES, PREFERS, "
    "STUDIES, MANAGES, MEMBER_OF, INTERESTED_IN, SKILLED_IN, "
    "CREATED, MAINTAINS, DEPENDS_ON, LOCATED_IN, PART_OF, "
    "SCHEDULED_FOR, DIAGNOSED_WITH, PRESCRIBED, TREATS, "
    "VEHICLE_IS, INSTALLED_ON, COMPATIBLE_WITH, CONNECTED_TO, "
    "COMMUNICATES_VIA, CONFIGURED_WITH, RUNS_ON, SERVES, "
    "RELATES_TO. "
    "Do NOT invent new relationship types outside this list. "
    "Use RELATES_TO as fallback when no specific type fits."
)

# Detect LLM backend at import time
_backend = detect_backend()
LLM_BACKEND = _backend


def build_mem0_config(backend: dict = None) -> dict:
    """Build Mem0 config using detected backend."""
    if backend is None:
        backend = _backend

    # Embedder: always fastembed by default, Voyage if key available
    if VOYAGE_API_KEY:
        embedder_config = {
            "provider": "openai",
            "config": {
                "model": "voyage-3-large",
                "api_key": VOYAGE_API_KEY,
                "openai_base_url": "https://api.voyageai.com/v1",
            },
        }
        embedding_dims = 1024
    else:
        embedder_config = {
            "provider": "fastembed",
            "config": {
                "model": "BAAI/bge-small-en-v1.5",  # 67MB, fast
            },
        }
        embedding_dims = 384  # bge-small-en-v1.5

    config = {
        "embedder": embedder_config,
        "vector_store": {
            "provider": "qdrant",
            "config": {
                "collection_name": "aletheia_memories",
                "host": QDRANT_HOST,
                "port": QDRANT_PORT,
                "embedding_model_dims": embedding_dims,
            },
        },
        "graph_store": {
            "provider": "neo4j",
            "config": {
                "url": NEO4J_URL,
                "username": NEO4J_USER,
                "password": NEO4J_PASSWORD,
                "base_label": True,
            },
        },
        "custom_prompt": GRAPH_EXTRACTION_PROMPT,
        "custom_fact_extraction_prompt": FACT_EXTRACTION_PROMPT,
    }

    # Add LLM config
    if backend["config"]:
        # Tier 1 api-key or Tier 2 ollama: use their native config
        config["llm"] = backend["config"]
    elif backend["provider"] == "anthropic-oauth":
        # OAuth mode: Mem0 needs an LLM config to init, but we'll replace
        # the LLM instance after creation. Use a placeholder.
        config["llm"] = {
            "provider": "anthropic",
            "config": {
                "model": backend.get("model", "claude-haiku-4-5-20251001"),
                "api_key": "oauth-placeholder-replaced-at-runtime",
                "temperature": 0.1,
                "max_tokens": 2000,
            },
        }

    return config


# Build the config for backward compat
MEM0_CONFIG = build_mem0_config()
