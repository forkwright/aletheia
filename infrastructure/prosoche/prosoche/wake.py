# Wake trigger — sends attention events to the Aletheia HTTP gateway
from __future__ import annotations

import hashlib
import json
import time
from collections import defaultdict

import httpx
from loguru import logger

from .scoring import NousScore

# Prosoche config uses "syn" but runtime config uses "main" for Syn (legacy)
AGENT_ID_MAP = {"syn": "main"}


def _signal_fingerprint(signals: list) -> str:
    """Create a hash of the urgent signal summaries to detect duplicates."""
    key = "|".join(sorted(s.summary for s in signals))
    return hashlib.md5(key.encode()).hexdigest()


class WakeBudget:
    def __init__(self, max_per_nous_per_hour: int = 2, max_total_per_hour: int = 6, cooldown_seconds: int = 300):
        self._max_per_nous = max_per_nous_per_hour
        self._max_total = max_total_per_hour
        self._cooldown = cooldown_seconds
        self._nous_wakes: dict[str, list[float]] = defaultdict(list)
        self._total_wakes: list[float] = []
        self._last_wake: dict[str, float] = {}
        # Track which signal sets have already been delivered per nous
        # Maps nous_id -> (fingerprint, delivery_time)
        self._delivered: dict[str, tuple[str, float]] = {}
        # Don't re-deliver the same signal set within this window (8 hours)
        # Static overdue tasks don't change until a human acts — no point
        # re-alerting hourly. New/changed signals get a fresh fingerprint.
        self._dedup_window = 28800.0

    def can_wake(self, nous_id: str) -> bool:
        now = time.monotonic()
        hour_ago = now - 3600

        self._total_wakes = [t for t in self._total_wakes if t > hour_ago]
        if len(self._total_wakes) >= self._max_total:
            return False

        self._nous_wakes[nous_id] = [t for t in self._nous_wakes[nous_id] if t > hour_ago]
        if len(self._nous_wakes[nous_id]) >= self._max_per_nous:
            return False

        last = self._last_wake.get(nous_id, 0)
        if now - last < self._cooldown:
            return False

        return True

    def is_duplicate(self, nous_id: str, signals: list) -> bool:
        """Check if this exact set of signals was already delivered recently."""
        fp = _signal_fingerprint(signals)
        prev = self._delivered.get(nous_id)
        if prev is None:
            return False
        prev_fp, prev_time = prev
        if fp == prev_fp and (time.monotonic() - prev_time) < self._dedup_window:
            return True
        return False

    def record_wake(self, nous_id: str, signals: list | None = None) -> None:
        now = time.monotonic()
        self._nous_wakes[nous_id].append(now)
        self._total_wakes.append(now)
        self._last_wake[nous_id] = now
        if signals:
            self._delivered[nous_id] = (_signal_fingerprint(signals), now)


async def trigger_wake(score: NousScore, config: dict) -> bool:
    gateway = config.get("gateway", {})
    token = gateway.get("token", "")
    base_url = gateway.get("url", "http://127.0.0.1:18789")
    base_url = base_url.replace("ws://", "http://").replace("wss://", "https://")

    urgent_items = [s for s in score.top_signals if s.urgency >= 0.8]
    if not urgent_items:
        return False

    text_parts = ["[prosoche] Attention needed:"]
    for signal in urgent_items[:3]:
        text_parts.append(f"- {signal.summary}")

    if score.staged_context:
        text_parts.append("")
        text_parts.append("Staged context available — check PROSOCHE.md for details.")

    event_text = "\n".join(text_parts)
    agent_id = AGENT_ID_MAP.get(score.nous_id, score.nous_id)

    payload = json.dumps({
        "agentId": agent_id,
        "message": event_text,
        "sessionKey": "prosoche",
    }).encode()

    url = f"{base_url}/api/sessions/send"
    headers = {
        "Content-Type": "application/json",
        "Authorization": f"Bearer {token}",
    }

    try:
        async with httpx.AsyncClient(timeout=30) as client:
            resp = await client.post(url, content=payload, headers=headers)
            if resp.status_code == 200:
                logger.info(f"Wake triggered for {score.nous_id}: {urgent_items[0].summary}")
                return True
            else:
                logger.warning(f"Wake failed for {score.nous_id}: HTTP {resp.status_code}")
                return False
    except httpx.HTTPStatusError as e:
        logger.warning(f"Wake failed for {score.nous_id}: HTTP {e.response.status_code}")
        return False
    except Exception as e:
        logger.error(f"Wake trigger error: {e}")
        return False
