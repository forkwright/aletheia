# Configuration loading and validation
from __future__ import annotations

import os
import re
from pathlib import Path
from typing import Any

import yaml


def _expand_env(obj: Any) -> Any:
    """Recursively expand ${VAR} references in strings using os.environ."""
    if isinstance(obj, str):
        return re.sub(r"\$\{(\w+)\}", lambda m: os.environ.get(m.group(1), m.group(0)), obj)
    if isinstance(obj, dict):
        return {k: _expand_env(v) for k, v in obj.items()}
    if isinstance(obj, list):
        return [_expand_env(v) for v in obj]
    return obj


def _default_config_path() -> Path:
    root = os.environ.get("ALETHEIA_ROOT")
    if root:
        return Path(root) / "infrastructure" / "prosoche" / "config.yaml"
    return Path(__file__).resolve().parent.parent / "config.yaml"


def load_config(path: Path | str | None = None) -> dict[str, Any]:
    if path is None:
        path = _default_config_path()
    path = Path(path)
    if not path.exists():
        raise FileNotFoundError(f"Config not found: {path}")
    with open(path) as f:
        raw = yaml.safe_load(f)
    return _expand_env(raw)


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
