# Mem0 configuration for Aletheia memory sidecar

import os

ANTHROPIC_API_KEY = os.environ.get("ANTHROPIC_API_KEY", "")
OLLAMA_BASE_URL = os.environ.get("OLLAMA_BASE_URL", "http://localhost:11434")
QDRANT_HOST = os.environ.get("QDRANT_HOST", "localhost")
QDRANT_PORT = int(os.environ.get("QDRANT_PORT", "6333"))
NEO4J_URL = os.environ.get("NEO4J_URL", "neo4j://localhost:7687")
NEO4J_USER = os.environ.get("NEO4J_USER", "neo4j")
NEO4J_PASSWORD = os.environ.get("NEO4J_PASSWORD", "aletheia-memory")

MEM0_CONFIG = {
    "llm": {
        "provider": "ollama",
        "config": {
            "model": "gemma3:4b",
            "temperature": 0.1,
            "ollama_base_url": OLLAMA_BASE_URL,
        },
    },
    "embedder": {
        "provider": "ollama",
        "config": {
            "model": "mxbai-embed-large",
            "ollama_base_url": OLLAMA_BASE_URL,
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
        },
    },
    "version": "v1.1",
}
