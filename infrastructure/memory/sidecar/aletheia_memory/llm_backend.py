"""Three-tier LLM backend detection for memory extraction.

Tier 1: Anthropic (OAuth token from gateway, or API key)
Tier 2: Ollama (local, auto-detect best model)
Tier 3: None (embedding-only mode, no fact extraction)
"""

import json
import logging
import os
from pathlib import Path
from typing import Optional

log = logging.getLogger("aletheia.memory.llm")

OAUTH_CREDS_PATH = Path.home() / ".aletheia" / "credentials" / "anthropic.json"
OLLAMA_URL = os.environ.get("OLLAMA_URL", "http://localhost:11434")

# Models suitable for structured JSON extraction, in preference order
OLLAMA_PREFERRED_MODELS = [
    "qwen2.5:7b",
    "qwen2.5:3b",
    "llama3.1:8b",
    "gemma2:9b",
    "mistral:7b",
    "phi3:3.8b",
]

HAIKU_MODEL = "claude-haiku-4-5-20251001"


class OAuthAnthropicLLM:
    """Anthropic LLM using OAuth token instead of API key.

    Subclasses Mem0's AnthropicLLM at runtime, overriding __init__
    to use auth_token. This lets Mem0's extraction pipeline work
    unchanged.
    """

    @staticmethod
    def create(auth_token: str, model: str = HAIKU_MODEL):
        """Create a Mem0-compatible AnthropicLLM using OAuth.

        Returns a patched instance that Mem0 can use directly.
        """
        from mem0.llms.anthropic import AnthropicLLM
        from mem0.configs.llms.anthropic import AnthropicConfig
        import anthropic as anthropic_sdk

        config = AnthropicConfig(
            model=model,
            temperature=0.1,
            max_tokens=2000,
            api_key="oauth-placeholder",  # Mem0 validates non-empty
        )

        instance = AnthropicLLM.__new__(AnthropicLLM)
        instance.config = config

        # Replace the client with one using auth_token
        instance.client = anthropic_sdk.Anthropic(
            auth_token=auth_token,
            default_headers={"anthropic-beta": "oauth-2025-04-20"},
        )

        log.info(f"Anthropic OAuth LLM initialized (model={model})")
        return instance


def read_oauth_token() -> Optional[str]:
    """Read OAuth token from gateway credentials file."""
    if not OAUTH_CREDS_PATH.exists():
        return None
    try:
        data = json.loads(OAUTH_CREDS_PATH.read_text())
        token = data.get("token")
        if token and len(token) > 20:
            return token
    except (json.JSONDecodeError, KeyError, OSError) as e:
        log.warning(f"Failed to read OAuth token: {e}")
    return None


def _check_anthropic_api_key() -> Optional[str]:
    """Check for ANTHROPIC_API_KEY env var."""
    key = os.environ.get("ANTHROPIC_API_KEY", "").strip()
    return key if key else None


def _check_ollama() -> Optional[str]:
    """Check if Ollama is running and find the best available model.

    Returns model name or None.
    """
    try:
        import httpx
        r = httpx.get(f"{OLLAMA_URL}/api/tags", timeout=3.0)
        if r.status_code != 200:
            return None
        data = r.json()
        available = {m["name"] for m in data.get("models", [])}

        # Try preferred models first
        for model in OLLAMA_PREFERRED_MODELS:
            if model in available:
                log.info(f"Ollama: found preferred model {model}")
                return model

        # Fall back to any model with reasonable size
        for m in data.get("models", []):
            name = m["name"]
            size_gb = m.get("size", 0) / (1024**3)
            if size_gb >= 1.5:  # At least ~3B params
                log.info(f"Ollama: using available model {name} ({size_gb:.1f}GB)")
                return name

        log.info("Ollama running but no suitable models found")
        return None
    except Exception:
        return None


def detect_backend() -> dict:
    """Detect the best available LLM backend.

    Returns a dict with:
        tier: 1, 2, or 3
        provider: "anthropic-oauth", "anthropic-apikey", "ollama", or "none"
        model: model name (or None for tier 3)
        config: partial Mem0 LLM config dict (or None for tier 3)
        llm_instance: pre-built LLM instance for OAuth (or None)
    """
    # Tier 1a: OAuth token
    token = read_oauth_token()
    if token:
        try:
            llm = OAuthAnthropicLLM.create(token, HAIKU_MODEL)
            # Quick validation: the client was created, that's enough
            log.info("Tier 1: Anthropic via OAuth token")
            return {
                "tier": 1,
                "provider": "anthropic-oauth",
                "model": HAIKU_MODEL,
                "config": None,  # We'll inject the LLM instance directly
                "llm_instance": llm,
                "oauth_token": token,
            }
        except Exception as e:
            log.warning(f"OAuth token found but LLM init failed: {e}")

    # Tier 1b: API key
    api_key = _check_anthropic_api_key()
    if api_key:
        log.info("Tier 1: Anthropic via API key")
        return {
            "tier": 1,
            "provider": "anthropic-apikey",
            "model": HAIKU_MODEL,
            "config": {
                "provider": "anthropic",
                "config": {
                    "model": HAIKU_MODEL,
                    "temperature": 0.1,
                    "max_tokens": 2000,
                    "api_key": api_key,
                },
            },
            "llm_instance": None,
            "oauth_token": None,
        }

    # Tier 2: Ollama
    ollama_model = _check_ollama()
    if ollama_model:
        log.info(f"Tier 2: Ollama with {ollama_model}")
        return {
            "tier": 2,
            "provider": "ollama",
            "model": ollama_model,
            "config": {
                "provider": "ollama",
                "config": {
                    "model": ollama_model,
                    "temperature": 0.1,
                    "max_tokens": 2000,
                    "ollama_base_url": OLLAMA_URL,
                },
            },
            "llm_instance": None,
            "oauth_token": None,
        }

    # Tier 3: No LLM
    log.warning("Tier 3: No LLM available. Embedding-only mode.")
    return {
        "tier": 3,
        "provider": "none",
        "model": None,
        "config": None,
        "llm_instance": None,
        "oauth_token": None,
    }


def refresh_oauth_token(current_backend: dict) -> dict:
    """Re-read OAuth token if it changed. Returns updated backend or same."""
    if current_backend["provider"] != "anthropic-oauth":
        return current_backend

    new_token = read_oauth_token()
    if not new_token:
        log.warning("OAuth token disappeared, falling back to re-detection")
        return detect_backend()

    if new_token != current_backend.get("oauth_token"):
        log.info("OAuth token rotated, re-creating Anthropic client")
        try:
            llm = OAuthAnthropicLLM.create(new_token, HAIKU_MODEL)
            current_backend["llm_instance"] = llm
            current_backend["oauth_token"] = new_token
        except Exception as e:
            log.warning(f"Token refresh failed: {e}, falling back")
            return detect_backend()

    return current_backend
