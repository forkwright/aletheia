# Hex signal — dashboard project run freshness and failure detection
from __future__ import annotations

import os
from datetime import datetime, timezone

import httpx
from loguru import logger

from . import Signal

HEX_API_BASE = "https://hc.hex.tech/api/v1"


async def collect(config: dict) -> list[Signal]:
    hex_config = config.get("signals", {}).get("hex", {})
    if not hex_config.get("enabled"):
        return []

    token = os.environ.get("HEX_API_TOKEN")
    if not token:
        logger.warning("HEX_API_TOKEN not set, skipping hex signal collection")
        return []

    projects = hex_config.get("projects", [])
    stale_hours = hex_config.get("stale_hours", 26)
    failure_urgency = hex_config.get("failure_urgency", 0.9)
    stale_urgency = hex_config.get("stale_urgency", 0.6)

    signals: list[Signal] = []
    headers = {"Authorization": f"Bearer {token}"}

    async with httpx.AsyncClient(timeout=15.0, headers=headers) as client:
        for project_id in projects:
            try:
                resp = await client.get(
                    f"{HEX_API_BASE}/project/{project_id}/runs",
                    params={"limit": 1},
                )
                if resp.status_code != 200:
                    logger.warning(f"Hex API returned {resp.status_code} for project {project_id}")
                    continue

                runs = resp.json()
                if not runs:
                    signals.append(Signal(
                        source="hex",
                        summary=f"No runs found for project {project_id[:8]}...",
                        urgency=stale_urgency,
                        relevant_nous=["chiron"],
                        details=f"project_id={project_id}",
                    ))
                    continue

                latest = runs[0] if isinstance(runs, list) else runs
                status = latest.get("status", "unknown")
                run_id = latest.get("runId", latest.get("id", "?"))

                if status in ("ERRORED", "error", "FAILED"):
                    signals.append(Signal(
                        source="hex",
                        summary=f"Hex project {project_id[:8]}... run failed ({status})",
                        urgency=failure_urgency,
                        relevant_nous=["chiron"],
                        details=f"project_id={project_id} run_id={run_id} status={status}",
                    ))
                    continue

                ended_at = latest.get("endTime") or latest.get("endedAt")
                if ended_at:
                    hours_ago = _hours_since(ended_at)
                    if hours_ago is not None and hours_ago > stale_hours:
                        signals.append(Signal(
                            source="hex",
                            summary=f"Hex project {project_id[:8]}... stale ({hours_ago:.0f}h since last run)",
                            urgency=stale_urgency,
                            relevant_nous=["chiron"],
                            details=f"project_id={project_id} last_run={ended_at} hours_ago={hours_ago:.1f}",
                        ))

            except httpx.TimeoutException:
                logger.warning(f"Hex API timeout for project {project_id}")
            except Exception as e:
                logger.warning(f"Hex signal failed for project {project_id}: {e}")

    return signals


def _hours_since(timestamp: str) -> float | None:
    try:
        if timestamp.endswith("Z"):
            dt = datetime.fromisoformat(timestamp.replace("Z", "+00:00"))
        else:
            dt = datetime.fromisoformat(timestamp)
        if dt.tzinfo is None:
            dt = dt.replace(tzinfo=timezone.utc)
        delta = datetime.now(timezone.utc) - dt
        return delta.total_seconds() / 3600
    except (ValueError, AttributeError):
        return None
