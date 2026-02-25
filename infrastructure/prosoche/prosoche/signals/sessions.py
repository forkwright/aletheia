# Sessions signal — agent response staleness via gateway API
from __future__ import annotations

import json
import os
from datetime import datetime, timezone

import httpx
from loguru import logger

from . import Signal

# Prosoche config uses "syn" but runtime config uses "main" for Syn
RUNTIME_TO_DISPLAY = {"main": "syn"}
DISPLAY_TO_RUNTIME = {"syn": "main"}


async def collect(config: dict) -> list[Signal]:
    sess_config = config.get("signals", {}).get("sessions", {})
    if not sess_config.get("enabled"):
        return []

    stale_hours = sess_config.get("stale_hours", 24)
    dormant_hours = sess_config.get("dormant_hours", 72)
    gateway = config.get("gateway", {})
    gateway_url = gateway.get("url", "http://127.0.0.1:18789")
    gateway_url = gateway_url.replace("ws://", "http://").replace("wss://", "https://")
    token = gateway.get("token", "")

    signals: list[Signal] = []
    now = datetime.now(timezone.utc)

    try:
        headers = {"Authorization": f"Bearer {token}"} if token else {}
        async with httpx.AsyncClient(timeout=10.0) as client:
            # Get agent list
            status_resp = await client.get(f"{gateway_url}/api/status", headers=headers)
            if status_resp.status_code != 200:
                logger.warning(f"Gateway status API returned {status_resp.status_code}")
                return []

            agent_ids = status_resp.json().get("agents", [])

            # Get all sessions
            sessions_resp = await client.get(f"{gateway_url}/api/sessions", headers=headers)
            if sessions_resp.status_code != 200:
                return []

            all_sessions = sessions_resp.json().get("sessions", [])

            for agent_id in agent_ids:
                display_name = RUNTIME_TO_DISPLAY.get(agent_id, agent_id)
                agent_sessions = [s for s in all_sessions if s.get("nousId") == agent_id]

                if not agent_sessions:
                    signals.append(Signal(
                        source="sessions",
                        summary=f"Agent {display_name} has no sessions",
                        urgency=0.4,
                        relevant_nous=["syn"],
                        details=f"agent={display_name} sessions=0",
                    ))
                    continue

                # Find most recent activity
                latest_ts = None
                for s in agent_sessions:
                    updated = s.get("updatedAt")
                    if updated:
                        try:
                            ts = datetime.fromisoformat(updated.replace("Z", "+00:00"))
                            if latest_ts is None or ts > latest_ts:
                                latest_ts = ts
                        except (ValueError, AttributeError):
                            pass

                if latest_ts is None:
                    continue

                hours_since = (now - latest_ts).total_seconds() / 3600

                if hours_since >= dormant_hours:
                    signals.append(Signal(
                        source="sessions",
                        summary=f"Agent {display_name} dormant — {hours_since:.0f}h since last activity",
                        urgency=0.5,
                        relevant_nous=["syn"],
                        details=f"agent={display_name} hours_since={hours_since:.1f} threshold={dormant_hours}h",
                    ))
                elif hours_since >= stale_hours:
                    signals.append(Signal(
                        source="sessions",
                        summary=f"Agent {display_name} stale — {hours_since:.0f}h since last activity",
                        urgency=0.3,
                        relevant_nous=["syn"],
                        details=f"agent={display_name} hours_since={hours_since:.1f} threshold={stale_hours}h",
                    ))

    except Exception as e:
        logger.warning(f"Session staleness check failed: {e}")

    return signals
