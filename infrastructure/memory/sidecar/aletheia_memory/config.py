# Mem0 configuration for Aletheia memory sidecar

import os

ANTHROPIC_API_KEY = os.environ.get("ANTHROPIC_API_KEY", "")
VOYAGE_API_KEY = os.environ.get("VOYAGE_API_KEY", "")
OLLAMA_BASE_URL = os.environ.get("OLLAMA_BASE_URL", "http://localhost:11434")
QDRANT_HOST = os.environ.get("QDRANT_HOST", "localhost")
QDRANT_PORT = int(os.environ.get("QDRANT_PORT", "6333"))
NEO4J_URL = os.environ.get("NEO4J_URL", "neo4j://localhost:7687")
NEO4J_USER = os.environ.get("NEO4J_USER", "neo4j")
NEO4J_PASSWORD = os.environ.get("NEO4J_PASSWORD", "aletheia-memory")

FACT_EXTRACTION_PROMPT = """\
You extract durable personal facts from conversations. Output JSON only.

EXTRACT — lasting facts about the user:
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
- Skip if uncertain — fewer quality memories beats many poor ones

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

MEM0_CONFIG = {
    "llm": {
        "provider": "anthropic",
        "config": {
            "model": "claude-haiku-4-5-20251001",
            "temperature": 0.1,
            "max_tokens": 2000,
            "api_key": ANTHROPIC_API_KEY,
        },
    },
    "embedder": {
        "provider": "openai",
        "config": {
            "model": "voyage-3-large",
            "api_key": VOYAGE_API_KEY,
            "openai_base_url": "https://api.voyageai.com/v1",
        },
    },
    "vector_store": {
        "provider": "qdrant",
        "config": {
            "collection_name": "aletheia_memories",
            "host": QDRANT_HOST,
            "port": QDRANT_PORT,
            "embedding_model_dims": 1024,
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
        "custom_prompt": GRAPH_EXTRACTION_PROMPT,
    },
    "custom_fact_extraction_prompt": FACT_EXTRACTION_PROMPT,
}
