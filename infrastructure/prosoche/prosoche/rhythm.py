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
        "has_digest": True,  # Triggers morning digest assembly
    },
    "midday_check": {
        "signals": [
            Signal(source="rhythm", summary="Midday: check task progress and afternoon calendar", urgency=0.3, relevant_nous=["syn"]),
        ],
        "window_minutes": 30,
        "has_digest": False,
    },
    "evening_review": {
        "signals": [
            Signal(source="rhythm", summary="Evening: review what happened today, pending items for tomorrow", urgency=0.3, relevant_nous=["syn"]),
        ],
        "window_minutes": 30,
        "has_digest": False,
    },
    "weekly_maintenance": {
        "signals": [
            Signal(source="rhythm", summary="Weekly: memory consolidation and agent audit", urgency=0.4, relevant_nous=["syn"]),
        ],
        "window_minutes": 60,
        "has_digest": False,
        "day_of_week": 6,  # Sunday only
    },
}


def get_rhythm_signals(config: dict) -> list[Signal]:
    rhythm_config = config.get("rhythm", {})
    if not rhythm_config:
        return []

    tz_name = config.get("quiet_hours", {}).get("timezone", "UTC")
    tz = zoneinfo.ZoneInfo(tz_name)
    now = datetime.now(tz)
    current_minutes = now.hour * 60 + now.minute

    signals = []
    for rhythm_name, time_str in rhythm_config.items():
        if rhythm_name not in RHYTHMS:
            continue

        rhythm_def = RHYTHMS[rhythm_name]

        # Day-of-week filter (for weekly rhythms)
        required_day = rhythm_def.get("day_of_week")
        if required_day is not None and now.weekday() != required_day:
            continue

        h, m = map(int, time_str.split(":"))
        target_minutes = h * 60 + m
        window = rhythm_def["window_minutes"]

        if target_minutes <= current_minutes < target_minutes + window:
            signals.extend(rhythm_def["signals"])

    return signals


def is_digest_time(config: dict) -> bool:
    """Check if we're in the morning digest window."""
    rhythm_config = config.get("rhythm", {})
    morning_time = rhythm_config.get("morning_prep")
    if not morning_time:
        return False

    tz_name = config.get("quiet_hours", {}).get("timezone", "UTC")
    tz = zoneinfo.ZoneInfo(tz_name)
    now = datetime.now(tz)
    current_minutes = now.hour * 60 + now.minute

    h, m = map(int, morning_time.split(":"))
    target_minutes = h * 60 + m
    window = RHYTHMS["morning_prep"]["window_minutes"]

    return target_minutes <= current_minutes < target_minutes + window


def is_weekly_maintenance_time(config: dict) -> bool:
    """Check if we're in the weekly maintenance window (Sunday)."""
    rhythm_config = config.get("rhythm", {})
    maint_time = rhythm_config.get("weekly_maintenance")
    if not maint_time:
        return False

    tz_name = config.get("quiet_hours", {}).get("timezone", "UTC")
    tz = zoneinfo.ZoneInfo(tz_name)
    now = datetime.now(tz)

    if now.weekday() != 6:  # Sunday
        return False

    current_minutes = now.hour * 60 + now.minute
    h, m = map(int, maint_time.split(":"))
    target_minutes = h * 60 + m
    window = RHYTHMS["weekly_maintenance"]["window_minutes"]

    return target_minutes <= current_minutes < target_minutes + window
