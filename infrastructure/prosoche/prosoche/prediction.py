# Predictive attention — learned temporal patterns per nous
from __future__ import annotations

import json
import zoneinfo
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path

from loguru import logger

from .signals import ContextBlock, Signal

BINS_PER_DAY = 24  # Hourly bins
DAYS_PER_WEEK = 7
MIN_OBSERVATIONS = 21  # 3 weeks before predictions are meaningful
HIGH_ACTIVITY_THRESHOLD = 0.7  # Top 30% of bins considered "peak"


def _bin_key(dt: datetime) -> tuple[int, int]:
    """Return (day_of_week 0=Mon, hour 0-23) for a local datetime."""
    return (dt.weekday(), dt.hour)


class ActivityModel:
    """Learned activity patterns: hourly bins per day-of-week per nous."""

    def __init__(self, data_dir: Path):
        self.data_dir = data_dir
        self.data_dir.mkdir(parents=True, exist_ok=True)
        self.observations: dict[str, dict[str, int]] = {}  # nous_id → {bin_key → count}
        self.total_days: dict[str, int] = {}
        self._load()

    def _path(self) -> Path:
        return self.data_dir / "activity_model.json"

    def _load(self) -> None:
        p = self._path()
        if not p.exists():
            return
        try:
            raw = json.loads(p.read_text())
            self.observations = raw.get("observations", {})
            self.total_days = raw.get("total_days", {})
        except Exception as e:
            logger.warning(f"Failed to load activity model: {e}")

    def _save(self) -> None:
        p = self._path()
        p.write_text(json.dumps({
            "observations": self.observations,
            "total_days": self.total_days,
            "updated_at": datetime.now(timezone.utc).isoformat(),
        }, indent=2))

    def record_activity(self, nous_id: str, dt: datetime) -> None:
        """Record that a nous was active at a given time."""
        key = f"{dt.weekday()}:{dt.hour}"
        if nous_id not in self.observations:
            self.observations[nous_id] = {}
        current = self.observations[nous_id].get(key, 0)
        self.observations[nous_id][key] = current + 1

    def record_day(self, nous_id: str) -> None:
        """Mark that we've observed another day for this nous."""
        self.total_days[nous_id] = self.total_days.get(nous_id, 0) + 1
        self._save()

    def has_enough_data(self, nous_id: str) -> bool:
        return self.total_days.get(nous_id, 0) >= MIN_OBSERVATIONS

    def predict_activity(self, nous_id: str, day: int, hour: int) -> float:
        """Return predicted activity level (0.0-1.0) for a specific bin."""
        if not self.has_enough_data(nous_id):
            return 0.5  # No prediction → neutral

        obs = self.observations.get(nous_id, {})
        key = f"{day}:{hour}"
        count = obs.get(key, 0)
        total = self.total_days.get(nous_id, 1)

        # Normalize against max observed frequency
        max_count = max(obs.values()) if obs else 1
        return min(1.0, count / max(max_count, 1))

    def get_peak_hours(self, nous_id: str, day: int) -> list[int]:
        """Return hours where activity is above threshold for a given day."""
        if not self.has_enough_data(nous_id):
            return []

        obs = self.observations.get(nous_id, {})
        max_count = max(obs.values()) if obs else 1
        threshold = max_count * HIGH_ACTIVITY_THRESHOLD

        peaks = []
        for hour in range(24):
            key = f"{day}:{hour}"
            if obs.get(key, 0) >= threshold:
                peaks.append(hour)
        return peaks

    def get_forecast(self, nous_id: str, tz_name: str = "America/Chicago") -> dict:
        """Generate a daily forecast for a nous."""
        tz = zoneinfo.ZoneInfo(tz_name)
        now = datetime.now(tz)
        today = now.weekday()

        if not self.has_enough_data(nous_id):
            return {
                "nous_id": nous_id,
                "ready": False,
                "days_observed": self.total_days.get(nous_id, 0),
                "days_needed": MIN_OBSERVATIONS,
            }

        peaks = self.get_peak_hours(nous_id, today)
        current_activity = self.predict_activity(nous_id, today, now.hour)

        return {
            "nous_id": nous_id,
            "ready": True,
            "day_name": now.strftime("%A"),
            "peak_hours": peaks,
            "current_activity_level": round(current_activity, 2),
            "is_peak_now": now.hour in peaks,
        }


def get_predictive_signals(model: ActivityModel, config: dict) -> list[Signal]:
    """Generate signals from learned activity patterns."""
    tz_name = config.get("quiet_hours", {}).get("timezone", "America/Chicago")
    tz = zoneinfo.ZoneInfo(tz_name)
    now = datetime.now(tz)

    nous_ids = list(config.get("nous", {}).keys())
    signals: list[Signal] = []

    for nous_id in nous_ids:
        if not model.has_enough_data(nous_id):
            continue

        forecast = model.get_forecast(nous_id, tz_name)
        if not forecast.get("ready"):
            continue

        # If we're approaching a peak hour (within 15 min), signal readiness
        next_hour = (now.hour + 1) % 24
        peaks = forecast.get("peak_hours", [])

        if next_hour in peaks and now.minute >= 45:
            signals.append(Signal(
                source="prediction",
                summary=f"Peak activity predicted for {nous_id} in ~{60 - now.minute}min",
                urgency=0.3,
                relevant_nous=[nous_id],
                context_blocks=[ContextBlock(
                    title=f"Activity Forecast: {nous_id}",
                    content=f"Today's peak hours: {', '.join(str(h) + ':00' for h in peaks)}\n"
                           f"Current activity level: {forecast['current_activity_level']}",
                    source="prediction",
                )],
            ))

    return signals
