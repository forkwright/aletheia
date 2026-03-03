# Morning digest — assembles weather + calendar + tasks + health into one staged block
from __future__ import annotations

import asyncio
import json
import os
from datetime import datetime, timedelta, timezone
from pathlib import Path

from loguru import logger

from .signals import ContextBlock, Signal

GCAL_BIN = str(Path(os.environ.get("ALETHEIA_ROOT", str(Path.home() / ".aletheia"))) / "shared" / "bin" / "gcal")


async def build_morning_digest(config: dict) -> Signal | None:
    """Assemble a comprehensive morning digest as a single signal with staged context."""
    parts: list[str] = []

    # Weather
    weather = await _get_weather()
    if weather:
        parts.append(f"**Weather:** {weather}")

    # Calendar (all calendars)
    cal_config = config.get("signals", {}).get("calendar", {})
    calendar_ids = cal_config.get("calendar_ids", {})
    cal_lines: list[str] = []
    for cal_name, cal_id in calendar_ids.items():
        events = await _get_today_events(cal_id)
        if events:
            cal_lines.append(f"*{cal_name}:*")
            for ev in events[:5]:
                cal_lines.append(f"  - {ev}")
    if cal_lines:
        parts.append("**Calendar:**\n" + "\n".join(cal_lines))
    else:
        parts.append("**Calendar:** No events today (or token expired)")

    # Tasks
    tasks = await _get_top_tasks()
    if tasks:
        task_lines = [f"  {i+1}. {t}" for i, t in enumerate(tasks[:7])]
        parts.append(f"**Tasks ({len(tasks)} pending):**\n" + "\n".join(task_lines))

    # Infrastructure health (quick check)
    health_lines = await _quick_health_check()
    if health_lines:
        parts.append("**Infrastructure:**\n" + "\n".join(f"  - {h}" for h in health_lines))
    else:
        parts.append("**Infrastructure:** All systems nominal")

    # Git
    prs = await _get_open_prs()
    if prs:
        parts.append(f"**Open PRs ({len(prs)}):**\n" + "\n".join(f"  - {p}" for p in prs[:5]))

    if not parts:
        return None

    content = "\n\n".join(parts)

    return Signal(
        source="rhythm",
        summary="Morning digest ready — review today's overview",
        urgency=0.6,
        relevant_nous=["syn"],
        details="morning_digest",
        context_blocks=[ContextBlock(
            title="Morning Digest",
            content=content,
            source="digest",
            expires_at=datetime.now(timezone.utc) + timedelta(hours=4),
        )],
    )


async def _get_weather() -> str | None:
    try:
        proc = await asyncio.create_subprocess_exec(
            "curl", "-s", "--max-time", "5",
            f"wttr.in/{os.environ.get('PROSOCHE_WEATHER_LOCATION', 'Austin+TX')}?format=%c+%t+%h+%w",
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        stdout, _ = await proc.communicate()
        if proc.returncode == 0:
            text = stdout.decode().strip()
            if text and "Unknown" not in text:
                return text
    except Exception:
        pass
    return None


async def _get_today_events(calendar_id: str) -> list[str]:
    try:
        proc = await asyncio.create_subprocess_exec(
            GCAL_BIN, "today", "-c", calendar_id,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        stdout, stderr = await proc.communicate()
        if proc.returncode != 0:
            err = stderr.decode().strip()
            if "invalid_grant" in err or "expired" in err:
                return ["⚠ Token expired — re-auth needed"]
            return []
        text = stdout.decode().strip()
        if not text or "no events" in text.lower():
            return []
        return [line.strip() for line in text.split("\n") if line.strip()][:10]
    except Exception:
        return []


async def _get_top_tasks() -> list[str]:
    try:
        proc = await asyncio.create_subprocess_exec(
            "task", "status:pending", "export",
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        stdout, _ = await proc.communicate()
        if proc.returncode != 0:
            return []
        tasks = json.loads(stdout.decode())
        # Sort by urgency (Taskwarrior computes this)
        tasks.sort(key=lambda t: t.get("urgency", 0), reverse=True)
        lines = []
        for t in tasks[:7]:
            desc = t.get("description", "?")
            proj = t.get("project", "")
            pri = t.get("priority", "")
            prefix = f"[{pri}]" if pri else ""
            suffix = f" ({proj})" if proj else ""
            lines.append(f"{prefix} {desc}{suffix}".strip())
        return lines
    except Exception:
        return []


async def _quick_health_check() -> list[str]:
    lines: list[str] = []
    # Disk usage
    try:
        proc = await asyncio.create_subprocess_exec(
            "df", "-h", "--output=target,pcent,avail", "/", os.environ.get("ALETHEIA_MOUNT_CHECK", "/"),  # Set ALETHEIA_MOUNT_CHECK to monitor a specific mount point
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        stdout, _ = await proc.communicate()
        for line in stdout.decode().strip().split("\n")[1:]:
            parts = line.split()
            if len(parts) >= 3:
                lines.append(f"Disk {parts[0]}: {parts[1].strip()} used, {parts[2].strip()} free")
    except Exception:
        pass

    # Docker health
    try:
        proc = await asyncio.create_subprocess_exec(
            "docker", "ps", "--filter", "health=unhealthy", "--format", "{{.Names}}",
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        stdout, _ = await proc.communicate()
        unhealthy = stdout.decode().strip()
        if unhealthy:
            lines.append(f"⚠ Unhealthy containers: {unhealthy}")
    except Exception:
        pass

    return lines


async def _get_open_prs() -> list[str]:
    try:
        proc = await asyncio.create_subprocess_exec(
            "gh", "pr", "list", "--repo", "forkwright/aletheia", "--state", "open",
            "--json", "number,title",
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        stdout, _ = await proc.communicate()
        if proc.returncode != 0:
            return []
        prs = json.loads(stdout.decode())
        return [f"#{pr['number']}: {pr['title'][:60]}" for pr in prs]
    except Exception:
        return []
