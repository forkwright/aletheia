#!/usr/bin/env python3
# Prosoche daemon — adaptive attention engine for Aletheia
from __future__ import annotations

import signal
import sys
import time
from pathlib import Path

import anyio
from loguru import logger

from .config import get_nous_ids, get_signal_interval, is_quiet_hours, load_config
from .prediction import ActivityModel, get_predictive_signals
from .rhythm import get_rhythm_signals
from .scoring import score_nous
from .signals import SignalBundle
from .signals import calendar as cal_signal
from .signals import health as health_signal
from .signals import memory as mem_signal
from .signals import tasks as task_signal
from .wake import WakeBudget, trigger_wake
from .writer import update_prosoche

logger.remove()
logger.add(sys.stderr, format="{time:HH:mm:ss} | {level:<7} | {message}", level="INFO")

COLLECTORS = {
    "calendar": cal_signal.collect,
    "tasks": task_signal.collect,
    "health": health_signal.collect,
    "memory": mem_signal.collect,
}


class ProsocheDaemon:
    def __init__(self, config_path: str | Path):
        self.config = load_config(config_path)
        self.nous_root = Path(self.config["nous_root"])
        self.nous_ids = get_nous_ids(self.config)
        self.bundle = SignalBundle()
        self.last_collection: dict[str, float] = {}
        self.running = True

        budget_cfg = self.config.get("budget", {})
        self.budget = WakeBudget(
            max_per_nous_per_hour=budget_cfg.get("max_wakes_per_nous_per_hour", 2),
            max_total_per_hour=budget_cfg.get("max_wakes_total_per_hour", 6),
            cooldown_seconds=budget_cfg.get("cooldown_after_wake_seconds", 300),
        )

        data_dir = Path(self.config.get("data_dir", "/mnt/ssd/aletheia/shared/prosoche"))
        self.activity_model = ActivityModel(data_dir)

    async def run(self) -> None:
        logger.info(f"Prosoche starting — {len(self.nous_ids)} nous, {len(COLLECTORS)} signals")

        async with anyio.create_task_group() as tg:
            tg.start_soon(self._main_loop)

    async def _sleep_interruptible(self, seconds: float) -> None:
        """Sleep in 5-second increments so SIGTERM isn't blocked."""
        remaining = seconds
        while remaining > 0 and self.running:
            await anyio.sleep(min(5.0, remaining))
            remaining -= 5.0

    async def _main_loop(self) -> None:
        while self.running:
            try:
                if is_quiet_hours(self.config):
                    logger.debug("Quiet hours — sleeping 15 min")
                    await self._sleep_interruptible(900)
                    continue

                await self._collect_signals()
                await self._evaluate_and_act()
                await self._sleep_interruptible(60)

            except Exception as e:
                logger.error(f"Main loop error: {e}")
                await self._sleep_interruptible(30)

    async def _collect_signals(self) -> None:
        now = time.monotonic()
        new_signals = []

        for name, collector in COLLECTORS.items():
            interval = get_signal_interval(self.config, name)
            last = self.last_collection.get(name, 0)

            if now - last < interval:
                continue

            try:
                signals = await collector(self.config)
                new_signals.extend(signals)
                self.last_collection[name] = now
                if signals:
                    logger.debug(f"Collected {len(signals)} {name} signals")
            except Exception as e:
                logger.warning(f"Signal collection failed for {name}: {e}")

        rhythm_signals = get_rhythm_signals(self.config)
        new_signals.extend(rhythm_signals)

        predictive_signals = get_predictive_signals(self.activity_model, self.config)
        new_signals.extend(predictive_signals)

        if new_signals:
            self.bundle = SignalBundle(signals=new_signals, collected_at=now)

    async def _evaluate_and_act(self) -> None:
        if not self.bundle.signals:
            return

        for nous_id in self.nous_ids:
            nous_config = self.config.get("nous", {}).get(nous_id, {})
            weights = nous_config.get("weights", {})

            score = score_nous(nous_id, self.bundle, weights)

            if score.top_signals:
                updated = update_prosoche(nous_id, score, self.nous_root)
                if updated:
                    logger.info(f"{nous_id}: score={score.score:.2f}, {len(score.top_signals)} items")

            if score.should_wake and self.budget.can_wake(nous_id):
                woke = await trigger_wake(score, self.config)
                if woke:
                    self.budget.record_wake(nous_id)
                    # Record activity for predictive model
                    import zoneinfo
                    from datetime import datetime
                    tz_name = self.config.get("quiet_hours", {}).get("timezone", "UTC")
                    tz = zoneinfo.ZoneInfo(tz_name)
                    self.activity_model.record_activity(nous_id, datetime.now(tz))

    def stop(self) -> None:
        self.running = False


def main() -> None:
    config_path = sys.argv[1] if len(sys.argv) > 1 else "/mnt/ssd/aletheia/infrastructure/prosoche/config.yaml"

    daemon = ProsocheDaemon(config_path)

    def handle_signal(signum, frame):
        logger.info("Shutting down...")
        daemon.stop()

    signal.signal(signal.SIGTERM, handle_signal)
    signal.signal(signal.SIGINT, handle_signal)

    anyio.run(daemon.run)


if __name__ == "__main__":
    main()
