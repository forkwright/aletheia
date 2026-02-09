# Wake trigger — sends system events to the Aletheia gateway
from __future__ import annotations

import asyncio
import time
from collections import defaultdict

from loguru import logger

from .scoring import NousScore


class WakeBudget:
    def __init__(self, max_per_nous_per_hour: int = 2, max_total_per_hour: int = 6, cooldown_seconds: int = 300):
        self._max_per_nous = max_per_nous_per_hour
        self._max_total = max_total_per_hour
        self._cooldown = cooldown_seconds
        self._nous_wakes: dict[str, list[float]] = defaultdict(list)
        self._total_wakes: list[float] = []
        self._last_wake: dict[str, float] = {}

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

    def record_wake(self, nous_id: str) -> None:
        now = time.monotonic()
        self._nous_wakes[nous_id].append(now)
        self._total_wakes.append(now)
        self._last_wake[nous_id] = now


async def trigger_wake(score: NousScore, config: dict) -> bool:
    gateway = config.get("gateway", {})
    token = gateway.get("token", "")

    urgent_items = [s for s in score.top_signals if s.urgency >= 0.8]
    if not urgent_items:
        return False

    text_parts = [f"Attention needed for {score.nous_id}:"]
    for signal in urgent_items[:3]:
        text_parts.append(f"- {signal.summary}")

    event_text = "\n".join(text_parts)

    cmd = [
        "aletheia", "system", "event",
        "--text", event_text,
        "--mode", "now",
    ]
    if token:
        cmd.extend(["--token", token])

    try:
        proc = await asyncio.create_subprocess_exec(
            *cmd,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        stdout, stderr = await proc.communicate()
        if proc.returncode == 0:
            logger.info(f"Wake triggered for {score.nous_id}: {urgent_items[0].summary}")
            return True
        else:
            logger.warning(f"Wake failed for {score.nous_id}: {stderr.decode()}")
            return False
    except Exception as e:
        logger.error(f"Wake trigger error: {e}")
        return False
