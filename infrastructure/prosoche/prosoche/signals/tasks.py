# Task signal — pending and overdue tasks from Taskwarrior via tw CLI
from __future__ import annotations

import asyncio
import json
from typing import Any

from loguru import logger

from . import Signal

TW_BIN = "/mnt/ssd/aletheia/shared/bin/tw"


async def collect(config: dict) -> list[Signal]:
    task_config = config.get("signals", {}).get("tasks", {})
    if not task_config.get("enabled"):
        return []

    overdue_urgency = task_config.get("overdue_urgency", 0.9)
    due_today_urgency = task_config.get("due_today_urgency", 0.6)
    project_nous = task_config.get("project_nous", {})

    signals = []

    overdue = await _query_tasks("status:pending", "+OVERDUE")
    for task in overdue:
        nous_id = _resolve_nous(task, project_nous)
        signals.append(Signal(
            source="tasks",
            summary=f"OVERDUE: {task.get('description', 'unknown task')}",
            urgency=overdue_urgency,
            relevant_nous=[nous_id, "syn"] if nous_id != "syn" else ["syn"],
            details=f"project:{task.get('project', '?')} priority:{task.get('priority', '?')}",
        ))

    due_today = await _query_tasks("status:pending", "due:today")
    for task in due_today:
        if any(s.details and task.get("description", "") in s.summary for s in signals):
            continue
        nous_id = _resolve_nous(task, project_nous)
        signals.append(Signal(
            source="tasks",
            summary=f"Due today: {task.get('description', 'unknown task')}",
            urgency=due_today_urgency,
            relevant_nous=[nous_id, "syn"] if nous_id != "syn" else ["syn"],
            details=f"project:{task.get('project', '?')} priority:{task.get('priority', '?')}",
        ))

    high_priority = await _query_tasks("status:pending", "priority:H", "-OVERDUE", "due.not:today")
    for task in high_priority[:5]:
        nous_id = _resolve_nous(task, project_nous)
        signals.append(Signal(
            source="tasks",
            summary=f"High priority: {task.get('description', 'unknown task')}",
            urgency=0.4,
            relevant_nous=[nous_id],
            details=f"project:{task.get('project', '?')}",
        ))

    return signals


async def _query_tasks(*filters: str) -> list[dict[str, Any]]:
    try:
        cmd = ["task", *filters, "export"]
        proc = await asyncio.create_subprocess_exec(
            *cmd,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        stdout, stderr = await proc.communicate()
        if proc.returncode != 0:
            return []
        return json.loads(stdout.decode())
    except Exception as e:
        logger.warning(f"Taskwarrior query failed: {e}")
        return []


def _resolve_nous(task: dict, project_nous: dict[str, str]) -> str:
    project = task.get("project", "")
    if project in project_nous:
        return project_nous[project]
    for prefix, nous_id in project_nous.items():
        if project.startswith(prefix):
            return nous_id
    return "syn"
