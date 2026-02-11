# Memory signal — new cross-nous memories from Mem0 sidecar
from __future__ import annotations

import httpx
from loguru import logger

from . import Signal


async def collect(config: dict) -> list[Signal]:
    mem_config = config.get("signals", {}).get("memory", {})
    if not mem_config.get("enabled"):
        return []

    sidecar_url = mem_config.get("sidecar_url", "http://127.0.0.1:8230")
    signals = []

    try:
        async with httpx.AsyncClient(timeout=10.0) as client:
            resp = await client.get(f"{sidecar_url}/health")
            if resp.status_code != 200:
                signals.append(Signal(
                    source="memory",
                    summary="Mem0 sidecar unhealthy",
                    urgency=0.6,
                    relevant_nous=["syn"],
                    details=f"Health check returned {resp.status_code}",
                ))
    except Exception as e:
        signals.append(Signal(
            source="memory",
            summary=f"Mem0 sidecar unreachable: {e}",
            urgency=0.5,
            relevant_nous=["syn"],
        ))

    return signals
