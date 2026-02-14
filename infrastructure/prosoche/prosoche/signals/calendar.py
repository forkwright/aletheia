# Calendar signal — upcoming events from Google Calendar via gcal CLI
from __future__ import annotations

import asyncio
import json
import time
from typing import Any

from loguru import logger

from . import Signal

GCAL_BIN = "/mnt/ssd/aletheia/shared/bin/gcal"


async def collect(config: dict) -> list[Signal]:
    cal_config = config.get("signals", {}).get("calendar", {})
    if not cal_config.get("enabled"):
        return []

    look_ahead = cal_config.get("look_ahead_minutes", 120)
    urgent_minutes = cal_config.get("urgent_minutes", 30)
    calendar_ids = cal_config.get("calendar_ids", {})

    signals = []
    for cal_name, cal_id in calendar_ids.items():
        try:
            events = await _fetch_events(cal_id, days=1)
            for event in events:
                minutes_until = _minutes_until(event)
                if minutes_until is None or minutes_until < -15:
                    continue
                if minutes_until > look_ahead:
                    continue

                if minutes_until <= urgent_minutes:
                    urgency = min(1.0, 0.7 + (urgent_minutes - minutes_until) / urgent_minutes * 0.3)
                    summary = f"URGENT: {event['title']} in {minutes_until}min"
                else:
                    urgency = 0.3 + (look_ahead - minutes_until) / look_ahead * 0.3
                    summary = f"{event['title']} in {minutes_until}min"

                relevant = _map_calendar_to_nous(cal_name, event, config)

                signals.append(Signal(
                    source="calendar",
                    summary=summary,
                    urgency=urgency,
                    relevant_nous=relevant,
                    details=f"{cal_name}: {event.get('title', '')} at {event.get('start', '')}",
                ))
        except Exception as e:
            logger.warning(f"Calendar signal failed for {cal_name}: {e}")

    return signals


async def _fetch_events(calendar_id: str, days: int = 1) -> list[dict[str, Any]]:
    proc = await asyncio.create_subprocess_exec(
        GCAL_BIN, "events", "-c", calendar_id, "-d", str(days),
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
    )
    stdout, stderr = await proc.communicate()
    if proc.returncode != 0:
        logger.warning(f"gcal failed: {stderr.decode()}")
        return []

    try:
        return json.loads(stdout.decode())
    except json.JSONDecodeError:
        text = stdout.decode().strip()
        if not text:
            return []
        events = []
        for line in text.split("\n"):
            line = line.strip()
            if not line:
                continue
            parts = line.split("|")
            if len(parts) >= 2:
                events.append({"title": parts[0].strip(), "start": parts[1].strip()})
        return events


def _minutes_until(event: dict) -> int | None:
    start = event.get("start", "")
    if not start:
        return None
    try:
        from datetime import datetime, timezone

        if "T" in start:
            if start.endswith("Z"):
                dt = datetime.fromisoformat(start.replace("Z", "+00:00"))
            else:
                dt = datetime.fromisoformat(start)
            if dt.tzinfo is None:
                import zoneinfo
                dt = dt.replace(tzinfo=zoneinfo.ZoneInfo("America/Chicago"))
            now = datetime.now(timezone.utc)
            return int((dt - now).total_seconds() / 60)
    except Exception:
        pass
    return None


def _map_calendar_to_nous(cal_name: str, event: dict, config: dict) -> list[str]:
    mapping = {
        "work": ["arbor", "syn"],
        "family": ["syl", "syn"],
        "personal": ["syn", "syl"],
    }
    return mapping.get(cal_name, ["syn"])
