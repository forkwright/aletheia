# Daily rhythm — time-based attention patterns
from __future__ import annotations

import zoneinfo
from datetime import datetime

from .signals import Signal

RHYTHMS = {
    "morning_prep": {
        "signals": [
            Signal(source="rhythm", summary="Morning: review calendar and tasks for today", urgency=0.5, relevant_nous=["syn", "syl"]),
            Signal(source="rhythm", summary="Morning: check overnight alerts and system health", urgency=0.4, relevant_nous=["syn"]),
        ],
        "window_minutes": 30,
    },
    "midday_check": {
        "signals": [
            Signal(source="rhythm", summary="Midday: check task progress and afternoon calendar", urgency=0.3, relevant_nous=["syn", "arbor", "eiron"]),
        ],
        "window_minutes": 30,
    },
    "evening_review": {
        "signals": [
            Signal(source="rhythm", summary="Evening: review what happened today, pending items for tomorrow", urgency=0.3, relevant_nous=["syn"]),
        ],
        "window_minutes": 30,
    },
}


def get_rhythm_signals(config: dict) -> list[Signal]:
    rhythm_config = config.get("rhythm", {})
    if not rhythm_config:
        return []

    tz_name = config.get("quiet_hours", {}).get("timezone", "America/Chicago")
    tz = zoneinfo.ZoneInfo(tz_name)
    now = datetime.now(tz)
    current_minutes = now.hour * 60 + now.minute

    signals = []
    for rhythm_name, time_str in rhythm_config.items():
        if rhythm_name not in RHYTHMS:
            continue

        h, m = map(int, time_str.split(":"))
        target_minutes = h * 60 + m
        window = RHYTHMS[rhythm_name]["window_minutes"]

        if target_minutes <= current_minutes < target_minutes + window:
            signals.extend(RHYTHMS[rhythm_name]["signals"])

    return signals
