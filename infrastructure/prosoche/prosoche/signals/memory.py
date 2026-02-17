# Memory signal — sidecar health + foresight signals from Neo4j
from __future__ import annotations

from datetime import datetime, timezone, timedelta

import httpx
from loguru import logger

from . import ContextBlock, Signal


async def collect(config: dict) -> list[Signal]:
    mem_config = config.get("signals", {}).get("memory", {})
    if not mem_config.get("enabled"):
        return []

    sidecar_url = mem_config.get("sidecar_url", "http://127.0.0.1:8230")
    signals: list[Signal] = []

    try:
        async with httpx.AsyncClient(timeout=10.0) as client:
            # Health check
            resp = await client.get(f"{sidecar_url}/health")
            if resp.status_code != 200:
                signals.append(Signal(
                    source="memory",
                    summary="Mem0 sidecar unhealthy",
                    urgency=0.6,
                    relevant_nous=["syn"],
                    details=f"Health check returned {resp.status_code}",
                ))

            # Foresight signals — active anticipatory context from Neo4j
            try:
                foresight_resp = await client.get(f"{sidecar_url}/foresight/active")
                if foresight_resp.status_code == 200:
                    data = foresight_resp.json()
                    for fs in data.get("signals", []):
                        entity = fs.get("entity", "unknown")
                        signal_text = fs.get("signal", "")
                        weight = fs.get("weight", 1.0)
                        expiry = fs.get("expiry")

                        # Map weight to urgency (weight 1.0+ = moderate, 5.0+ = high)
                        urgency = min(0.3 + (weight * 0.1), 0.9)

                        # Build expiry datetime for context block
                        expires_at = None
                        if expiry:
                            try:
                                expires_at = datetime.fromisoformat(expiry.replace("Z", "+00:00"))
                            except (ValueError, AttributeError):
                                expires_at = datetime.now(timezone.utc) + timedelta(hours=24)

                        signals.append(Signal(
                            source="memory",
                            summary=f"Foresight: {entity} — {signal_text}",
                            urgency=urgency,
                            relevant_nous=[],  # All agents see foresight
                            details=f"Weight: {weight}, entity: {entity}",
                            context_blocks=[ContextBlock(
                                title=f"Foresight: {entity}",
                                content=signal_text,
                                source="foresight",
                                expires_at=expires_at,
                            )],
                        ))
            except Exception as e:
                logger.debug(f"Foresight query failed (non-critical): {e}")

            # Discovery candidates — cross-domain bridges and unexpected connections
            try:
                disc_resp = await client.get(f"{sidecar_url}/discovery/candidates", params={"limit": 10})
                if disc_resp.status_code == 200:
                    data = disc_resp.json()
                    candidates = data.get("candidates", [])
                    bridges = [c for c in candidates if c.get("type") == "cross_community_bridge"]
                    hubs = [c for c in candidates if c.get("type") == "high_betweenness_hub"]

                    if bridges:
                        bridge_lines = []
                        for b in bridges[:5]:
                            bridge_lines.append(
                                f"- **{b['entity_a']}** ↔ **{b['entity_b']}** "
                                f"(communities {b.get('community_a', '?')} and {b.get('community_b', '?')})"
                            )
                        signals.append(Signal(
                            source="memory",
                            summary=f"{len(bridges)} cross-domain bridges discovered",
                            urgency=0.4,
                            relevant_nous=[],
                            details=f"Bridges: {len(bridges)}, Hubs: {len(hubs)}",
                            context_blocks=[ContextBlock(
                                title="Cross-Domain Discoveries",
                                content=(
                                    "The knowledge graph found unexpected connections between domains:\n"
                                    + "\n".join(bridge_lines)
                                    + "\n\nThese may reveal non-obvious relationships worth exploring."
                                ),
                                source="discovery",
                                expires_at=datetime.now(timezone.utc) + timedelta(hours=12),
                            )],
                        ))
            except Exception as e:
                logger.debug(f"Discovery query failed (non-critical): {e}")

            # Evolution stats — memory health
            try:
                evo_resp = await client.get(f"{sidecar_url}/evolution/stats")
                if evo_resp.status_code == 200:
                    data = evo_resp.json()
                    decaying = data.get("decaying_memories", 0)
                    evolutions = data.get("evolutions", 0)

                    if decaying > 10:
                        signals.append(Signal(
                            source="memory",
                            summary=f"{decaying} memories decaying — may need attention",
                            urgency=0.3,
                            relevant_nous=["syn"],
                            details=f"Decaying: {decaying}, Evolutions: {evolutions}",
                        ))
            except Exception as e:
                logger.debug(f"Evolution stats query failed (non-critical): {e}")

    except Exception as e:
        signals.append(Signal(
            source="memory",
            summary=f"Mem0 sidecar unreachable: {e}",
            urgency=0.5,
            relevant_nous=["syn"],
        ))

    return signals
