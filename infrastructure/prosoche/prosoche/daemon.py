#!/usr/bin/env python3
# Prosoche daemon — adaptive attention engine for Aletheia
from __future__ import annotations

import hashlib
import os
import signal
import sys
import time
from pathlib import Path

import anyio
import httpx
from loguru import logger

from .config import get_nous_ids, get_signal_interval, is_quiet_hours, load_config
from .prediction import ActivityModel, get_predictive_signals
from .digest import build_morning_digest
from .rhythm import get_rhythm_signals, is_digest_time, is_weekly_maintenance_time
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

# Lazy-loaded optional collectors — registered at startup if config enables them
_OPTIONAL_COLLECTORS: dict[str, str] = {
    "git": ".signals.git",
    "nas": ".signals.nas",
    "credentials": ".signals.credentials",
    "sessions": ".signals.sessions",
}


def _register_optional_collectors(config: dict) -> None:
    """Import and register optional signal collectors that are enabled in config."""
    import importlib

    for name, module_path in _OPTIONAL_COLLECTORS.items():
        signal_config = config.get("signals", {}).get(name, {})
        if signal_config.get("enabled"):
            try:
                mod = importlib.import_module(module_path, package="prosoche")
                COLLECTORS[name] = mod.collect
                logger.info(f"Registered signal collector: {name}")
            except ImportError as e:
                logger.warning(f"Failed to import signal collector {name}: {e}")


class ProsocheDaemon:
    def __init__(self, config_path: str | Path | None = None):
        self.config = load_config(config_path)
        self.nous_root = Path(self.config["nous_root"])
        self.nous_ids = get_nous_ids(self.config)
        self.bundle = SignalBundle()
        self.last_collection: dict[str, float] = {}
        self.running = True

        # Track which rhythm signals have fired today to prevent repeats
        self._rhythm_fired_today: set[str] = set()
        self._rhythm_day: int = -1  # day-of-year tracker for daily reset

        # Track digest and weekly maintenance to fire once per window
        self._digest_fired_today: bool = False
        self._weekly_maint_fired: int = -1  # week-of-year tracker

        # Track broadcast fingerprints to prevent duplicate posts
        self._broadcast_sent: dict[str, tuple[str, float]] = {}
        self._broadcast_dedup_window = 3600.0  # 1 hour

        budget_cfg = self.config.get("budget", {})
        self.budget = WakeBudget(
            max_per_nous_per_hour=budget_cfg.get("max_wakes_per_nous_per_hour", 2),
            max_total_per_hour=budget_cfg.get("max_wakes_total_per_hour", 6),
            cooldown_seconds=budget_cfg.get("cooldown_after_wake_seconds", 300),
        )

        fallback = os.environ.get("ALETHEIA_ROOT", str(Path(__file__).resolve().parents[3]))
        data_dir = Path(self.config.get("data_dir", f"{fallback}/shared/prosoche"))
        self.activity_model = ActivityModel(data_dir)

        # Register optional signal collectors
        _register_optional_collectors(self.config)

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

    def _seconds_until_next_collection(self) -> float:
        """Calculate how long until the next signal collector is due."""
        now = time.monotonic()
        min_wait = 300.0  # Cap at 5 minutes even if nothing is due

        for name in COLLECTORS:
            interval = get_signal_interval(self.config, name)
            last = self.last_collection.get(name, 0)
            elapsed = now - last
            remaining = max(0, interval - elapsed)
            min_wait = min(min_wait, remaining)

        return max(5.0, min_wait)  # Floor at 5s to prevent busy-loop

    async def _main_loop(self) -> None:
        while self.running:
            try:
                if is_quiet_hours(self.config):
                    logger.debug("Quiet hours — sleeping 15 min")
                    await self._sleep_interruptible(900)
                    continue

                await self._collect_signals()
                await self._evaluate_and_act()

                # Smart sleep: wait until next collector is due
                wait = self._seconds_until_next_collection()
                logger.debug(f"Next collection in {wait:.0f}s")
                await self._sleep_interruptible(wait)

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

        # Daily reset for rhythm dedup and digest flag
        import datetime as _dt

        today = _dt.date.today().timetuple().tm_yday
        if today != self._rhythm_day:
            self._rhythm_fired_today.clear()
            self._digest_fired_today = False
            self._rhythm_day = today

        # Rhythm signals with daily dedup
        rhythm_signals = get_rhythm_signals(self.config)
        for rs in rhythm_signals:
            key = rs.summary
            if key not in self._rhythm_fired_today:
                new_signals.append(rs)
                self._rhythm_fired_today.add(key)

        # Morning digest — fire once per day during morning window
        if is_digest_time(self.config) and not self._digest_fired_today:
            try:
                digest_signal = await build_morning_digest(self.config)
                if digest_signal:
                    new_signals.append(digest_signal)
                    self._digest_fired_today = True
                    logger.info("Morning digest assembled")
            except Exception as e:
                logger.warning(f"Morning digest failed: {e}")

        # Weekly maintenance
        current_week = _dt.date.today().isocalendar()[1]
        if is_weekly_maintenance_time(self.config) and self._weekly_maint_fired != current_week:
            await self._run_weekly_maintenance()
            self._weekly_maint_fired = current_week

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

                # Broadcast high-urgency signals to the runtime blackboard (with dedup)
                urgent = [s for s in score.top_signals if s.urgency >= 0.7]
                if urgent:
                    await self._post_broadcasts(nous_id, urgent)

            if score.should_wake and self.budget.can_wake(nous_id):
                urgent = [s for s in score.top_signals if s.urgency >= 0.8]
                if self.budget.is_duplicate(nous_id, urgent):
                    logger.debug(f"{nous_id}: skipping wake — duplicate signal set")
                else:
                    self.budget.record_wake(nous_id, signals=urgent)
                    woke = await trigger_wake(score, self.config)
                    if not woke:
                        logger.warning(f"{nous_id}: wake delivery failed — will retry after dedup window")
                    # Record activity for predictive model
                    import zoneinfo
                    from datetime import datetime

                    tz_name = self.config.get("quiet_hours", {}).get("timezone", "UTC")
                    tz = zoneinfo.ZoneInfo(tz_name)
                    now_local = datetime.now(tz)
                    self.activity_model.record_activity(nous_id, now_local)
                    self.activity_model.maybe_record_day(nous_id, now_local)

    def _broadcast_fingerprint(self, nous_id: str, signals: list) -> str:
        key = f"{nous_id}|" + "|".join(sorted(s.summary for s in signals))
        return hashlib.md5(key.encode()).hexdigest()

    async def _post_broadcasts(self, nous_id: str, signals: list) -> None:
        # Dedup: don't re-broadcast the same signal set within the window
        fp = self._broadcast_fingerprint(nous_id, signals)
        now = time.monotonic()
        prev = self._broadcast_sent.get(nous_id)
        if prev:
            prev_fp, prev_time = prev
            if fp == prev_fp and (now - prev_time) < self._broadcast_dedup_window:
                return

        gateway = self.config.get("gateway", {})
        gateway_url = gateway.get("url", "http://127.0.0.1:18789")
        gateway_url = gateway_url.replace("ws://", "http://").replace("wss://", "https://")

        try:
            async with httpx.AsyncClient(timeout=5.0) as client:
                for sig in signals[:3]:
                    await client.post(
                        f"{gateway_url}/api/blackboard",
                        json={
                            "key": f"broadcast:{sig.source}:{nous_id}",
                            "value": sig.summary,
                            "author": "prosoche",
                            "ttl_seconds": 1800,
                        },
                    )
            self._broadcast_sent[nous_id] = (fp, now)
            logger.debug(f"Posted {min(len(signals), 3)} broadcasts for {nous_id}")
        except Exception as e:
            logger.debug(f"Broadcast post failed (non-critical): {e}")

    async def _run_weekly_maintenance(self) -> None:
        """Run weekly maintenance tasks: memory consolidation + agent audit."""
        import asyncio as _aio
        import json

        logger.info("Running weekly maintenance")

        # Memory consolidation
        consolidate_bin = "/mnt/ssd/aletheia/shared/bin/consolidate-memory"
        if os.path.exists(consolidate_bin):
            try:
                proc = await _aio.create_subprocess_exec(
                    consolidate_bin, "--all",
                    stdout=_aio.subprocess.PIPE,
                    stderr=_aio.subprocess.PIPE,
                )
                stdout, _ = await proc.communicate()
                logger.info(f"Memory consolidation complete (exit {proc.returncode})")
                if stdout:
                    logger.debug(stdout.decode().strip()[:200])
            except Exception as e:
                logger.warning(f"Memory consolidation failed: {e}")
        else:
            logger.debug("consolidate-memory not found — skipping")

        # Agent audit via nous-health
        try:
            proc = await _aio.create_subprocess_exec(
                "/mnt/ssd/aletheia/shared/bin/nous-health", "--json",
                stdout=_aio.subprocess.PIPE,
                stderr=_aio.subprocess.PIPE,
            )
            stdout, _ = await proc.communicate()
            if proc.returncode == 0:
                data = json.loads(stdout.decode())
                agents = data.get("agents", {})
                stale = [n for n, info in agents.items() if info.get("status") in ("stale", "dormant")]
                if stale:
                    logger.info(f"Weekly audit: stale agents: {', '.join(stale)}")
                else:
                    logger.info("Weekly audit: all agents healthy")
        except Exception as e:
            logger.warning(f"Agent audit failed: {e}")

    def stop(self) -> None:
        self.running = False


def main() -> None:
    config_path = sys.argv[1] if len(sys.argv) > 1 else None

    daemon = ProsocheDaemon(config_path)

    def handle_signal(signum, frame):
        logger.info("Shutting down...")
        daemon.stop()

    signal.signal(signal.SIGTERM, handle_signal)
    signal.signal(signal.SIGINT, handle_signal)

    anyio.run(daemon.run)


if __name__ == "__main__":
    main()
