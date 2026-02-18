# Redshift signal — cluster health via AWS Data API (failed and long-running queries)
from __future__ import annotations

import asyncio
import json
from datetime import datetime, timezone

from loguru import logger

from . import Signal


async def collect(config: dict) -> list[Signal]:
    rs_config = config.get("signals", {}).get("redshift", {})
    if not rs_config.get("enabled"):
        return []

    cluster = rs_config.get("cluster", "")
    if not cluster:
        logger.warning("Redshift cluster not configured, skipping signal collection")
        return []

    failed_urgency = rs_config.get("failed_query_urgency", 0.9)
    long_running_seconds = rs_config.get("long_running_seconds", 300)
    long_running_urgency = rs_config.get("long_running_urgency", 0.7)

    signals: list[Signal] = []

    failed = await _list_statements(cluster, status="FAILED", limit=5)
    for stmt in failed:
        stmt_id = stmt.get("Id", "?")
        query_string = stmt.get("QueryString", "")
        updated = stmt.get("UpdatedAt", "")
        preview = query_string[:120] + "..." if len(query_string) > 120 else query_string
        signals.append(Signal(
            source="redshift",
            summary=f"Redshift query failed: {preview[:60]}",
            urgency=failed_urgency,
            relevant_nous=["chiron"],
            details=f"statement_id={stmt_id} updated={updated} sql={preview}",
        ))

    started = await _list_statements(cluster, status="STARTED")
    now = datetime.now(timezone.utc)
    for stmt in started:
        created_str = stmt.get("CreatedAt", "")
        duration = _seconds_since(created_str, now)
        if duration is not None and duration > long_running_seconds:
            stmt_id = stmt.get("Id", "?")
            query_string = stmt.get("QueryString", "")
            preview = query_string[:120] + "..." if len(query_string) > 120 else query_string
            minutes = duration / 60
            signals.append(Signal(
                source="redshift",
                summary=f"Redshift query running {minutes:.0f}min: {preview[:50]}",
                urgency=long_running_urgency,
                relevant_nous=["chiron"],
                details=f"statement_id={stmt_id} duration={minutes:.1f}min sql={preview}",
            ))

    return signals


async def _list_statements(cluster: str, status: str, limit: int = 10) -> list[dict]:
    try:
        cmd = [
            "aws", "redshift-data", "list-statements",
            "--cluster-identifier", cluster,
            "--status", status,
        ]
        if status == "FAILED":
            cmd.extend(["--max-results", str(limit)])

        proc = await asyncio.create_subprocess_exec(
            *cmd,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        stdout, stderr = await proc.communicate()

        if proc.returncode != 0:
            err = stderr.decode().strip()
            logger.warning(f"AWS CLI failed (status={status}): {err}")
            return []

        data = json.loads(stdout.decode())
        return data.get("Statements", [])
    except FileNotFoundError:
        logger.warning("AWS CLI not found, skipping redshift signal collection")
        return []
    except json.JSONDecodeError as e:
        logger.warning(f"Failed to parse AWS CLI output: {e}")
        return []
    except Exception as e:
        logger.warning(f"Redshift list-statements failed (status={status}): {e}")
        return []


def _seconds_since(timestamp: str, now: datetime) -> float | None:
    if not timestamp:
        return None
    try:
        if isinstance(timestamp, str):
            if timestamp.endswith("Z"):
                dt = datetime.fromisoformat(timestamp.replace("Z", "+00:00"))
            else:
                dt = datetime.fromisoformat(timestamp)
        elif isinstance(timestamp, (int, float)):
            dt = datetime.fromtimestamp(timestamp, tz=timezone.utc)
        else:
            return None
        if dt.tzinfo is None:
            dt = dt.replace(tzinfo=timezone.utc)
        return (now - dt).total_seconds()
    except (ValueError, AttributeError, OSError):
        return None
