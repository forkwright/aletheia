# NAS signal — ping, SSH port probe, mount check for Synology health
from __future__ import annotations

import asyncio
import os

from loguru import logger

from . import Signal


async def collect(config: dict) -> list[Signal]:
    nas_config = config.get("signals", {}).get("nas", {})
    if not nas_config.get("enabled"):
        return []

    host = nas_config.get("host", "192.168.0.120")
    ssh_port = nas_config.get("ssh_port", 22)
    mounts = nas_config.get("mounts", ["/mnt/nas/Media", "/mnt/nas/docker", "/mnt/nas/photos"])
    signals: list[Signal] = []

    # Ping check
    reachable = await _ping(host)
    if not reachable:
        signals.append(Signal(
            source="nas",
            summary=f"NAS unreachable ({host}) — ping failed",
            urgency=0.95,
            relevant_nous=["syn"],
            details=f"host={host} ping=failed",
        ))
        return signals  # If ping fails, no point checking SSH or mounts

    # SSH port probe
    ssh_ok = await _check_port(host, ssh_port)
    if not ssh_ok:
        signals.append(Signal(
            source="nas",
            summary=f"NAS SSH port {ssh_port} refused ({host})",
            urgency=0.5,
            relevant_nous=["syn"],
            details=f"host={host} port={ssh_port} status=refused",
        ))

    # Mount checks
    for mount in mounts:
        if not os.path.ismount(mount):
            signals.append(Signal(
                source="nas",
                summary=f"NAS mount missing: {mount}",
                urgency=0.8,
                relevant_nous=["syn"],
                details=f"mount={mount} status=not_mounted",
            ))
        else:
            # Check if mount is stale (can't stat)
            try:
                os.statvfs(mount)
            except OSError:
                signals.append(Signal(
                    source="nas",
                    summary=f"NAS mount stale: {mount}",
                    urgency=0.85,
                    relevant_nous=["syn"],
                    details=f"mount={mount} status=stale",
                ))

    return signals


async def _ping(host: str) -> bool:
    try:
        proc = await asyncio.create_subprocess_exec(
            "ping", "-c", "1", "-W", "3", host,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        await proc.communicate()
        return proc.returncode == 0
    except Exception:
        return False


async def _check_port(host: str, port: int) -> bool:
    """TCP connect probe — returns True if port is open."""
    try:
        _, writer = await asyncio.wait_for(
            asyncio.open_connection(host, port),
            timeout=5.0,
        )
        writer.close()
        await writer.wait_closed()
        return True
    except (ConnectionRefusedError, asyncio.TimeoutError, OSError):
        return False
