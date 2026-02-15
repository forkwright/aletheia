# Configuration loading and validation
from __future__ import annotations

from pathlib import Path
from typing import Any

import yaml


def load_config(path: Path | str = "/mnt/ssd/aletheia/infrastructure/prosoche/config.yaml") -> dict[str, Any]:
    path = Path(path)
    if not path.exists():
        raise FileNotFoundError(f"Config not found: {path}")
    with open(path) as f:
        return yaml.safe_load(f)


def get_nous_ids(config: dict) -> list[str]:
    return list(config.get("nous", {}).keys())


def get_signal_interval(config: dict, signal_name: str) -> int:
    return config.get("signals", {}).get(signal_name, {}).get("interval_seconds", 300)


def is_quiet_hours(config: dict) -> bool:
    from datetime import datetime

    import zoneinfo

    qh = config.get("quiet_hours", {})
    if not qh.get("start") or not qh.get("end"):
        return False

    tz = zoneinfo.ZoneInfo(qh.get("timezone", "UTC"))
    now = datetime.now(tz)
    current = now.hour * 60 + now.minute

    start_h, start_m = map(int, qh["start"].split(":"))
    end_h, end_m = map(int, qh["end"].split(":"))
    start = start_h * 60 + start_m
    end = end_h * 60 + end_m

    if start > end:
        return current >= start or current < end
    return start <= current < end
